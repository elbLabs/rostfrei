use crate::rental_fleet::{
    AddBicycle, BicycleId, BicycleRented, FleetId, RentBicycle, ReturnBicycle, TransferBicycle,
};
use async_trait::async_trait;
use rostfrei::{
    Command, CommittedDomainEvent, EncodedIntegrationMessage, IntegrationEvent,
    IntegrationEventMapper,
};
use rostfrei_fixtures::{Fixture, FixtureApplyReport};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CommandAddress, ConsumerConfig, ContractError,
    DeliveryDisposition, IntegrationEventAddress, MessageDelivery, MessageHandler,
    QuarantineReason, RetryDelay, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsDomainEventConsumerConfig, NatsEventStore,
    NatsEventStoreConfig,
};
use rostfrei_tracer::{
    CommandTransport, CorrelationObserver, NatsRuntime, NatsRuntimeConfig, TestScenarioReset,
    TestScenarioResetError,
};
use serde::{Deserialize, Serialize};
use std::{env, sync::Arc, time::Duration};

#[cfg(test)]
mod tests;

pub const BOUNDED_CONTEXT_NAME: &str = "bike-rental";
pub const APPLICATION_NAME: &str = "bike-rental";
pub const BICYCLE_RENTAL_STARTED_EVENT_NAME: &str = "bicycle-rental-started";
const COMMAND_SUBSCRIPTION_GENERATION: u32 = 2;
pub type BikeRentalNatsError = rostfrei_tracer::NatsHostError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BikeRentalNatsResourceLimits {
    messaging_stream: i64,
    event_store_stream: i64,
    event_store_event: usize,
}
impl BikeRentalNatsResourceLimits {
    pub const fn new(
        messaging_stream_max_bytes: i64,
        event_store_max_stream_bytes: i64,
        event_store_max_event_bytes: usize,
    ) -> Self {
        Self {
            messaging_stream: messaging_stream_max_bytes,
            event_store_stream: event_store_max_stream_bytes,
            event_store_event: event_store_max_event_bytes,
        }
    }
    pub fn from_env() -> Result<Self, BikeRentalNatsError> {
        let defaults = Self::default();
        Ok(Self::new(
            env_limit(
                "ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES",
                defaults.messaging_stream,
            )?,
            env_limit(
                "ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES",
                defaults.event_store_stream,
            )?,
            env_limit(
                "ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES",
                defaults.event_store_event,
            )?,
        ))
    }
    pub const fn messaging_stream_max_bytes(self) -> i64 {
        self.messaging_stream
    }
    pub const fn event_store_max_stream_bytes(self) -> i64 {
        self.event_store_stream
    }
    pub const fn event_store_max_event_bytes(self) -> usize {
        self.event_store_event
    }
}
impl Default for BikeRentalNatsResourceLimits {
    fn default() -> Self {
        Self::new(
            64 * 1024 * 1024,
            rostfrei_nats::DEFAULT_EVENT_STORE_MAX_STREAM_BYTES,
            rostfrei_nats::DEFAULT_EVENT_STORE_MAX_EVENT_BYTES,
        )
    }
}
fn env_limit<T: std::str::FromStr + PartialOrd + Default>(
    name: &str,
    default: T,
) -> Result<T, BikeRentalNatsError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<T>()
            .ok()
            .filter(|value| *value > T::default())
            .ok_or_else(|| {
                BikeRentalNatsError::Configuration(format!(
                    "{name} must be a positive integer byte count"
                ))
            }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(BikeRentalNatsError::Configuration(format!(
            "{name}: {error}"
        ))),
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BikeRentalCommand {
    RentBicycle,
    ReturnBicycle,
    AddBicycle,
    TransferBicycle,
}
impl BikeRentalCommand {
    pub const fn command_name(self) -> &'static str {
        match self {
            Self::RentBicycle => RentBicycle::LOCAL_ID,
            Self::ReturnBicycle => ReturnBicycle::LOCAL_ID,
            Self::AddBicycle => AddBicycle::LOCAL_ID,
            Self::TransferBicycle => TransferBicycle::LOCAL_ID,
        }
    }
    pub const fn schema_version(self) -> u32 {
        match self {
            Self::RentBicycle => RentBicycle::SCHEMA_VERSION,
            Self::ReturnBicycle => ReturnBicycle::SCHEMA_VERSION,
            Self::AddBicycle => AddBicycle::SCHEMA_VERSION,
            Self::TransferBicycle => TransferBicycle::SCHEMA_VERSION,
        }
    }
}
#[derive(Clone)]
pub struct BikeRentalCommandRoute {
    command: BikeRentalCommand,
    address: CommandAddress,
    consumer: ConsumerConfig<CommandAddress>,
}
impl BikeRentalCommandRoute {
    pub const fn command(&self) -> BikeRentalCommand {
        self.command
    }
    pub const fn address(&self) -> &CommandAddress {
        &self.address
    }
    pub const fn consumer(&self) -> &ConsumerConfig<CommandAddress> {
        &self.consumer
    }
}
#[derive(Clone)]
pub struct BikeRentalIntegrationEventRoute {
    address: IntegrationEventAddress,
    consumer: ConsumerConfig<IntegrationEventAddress>,
}
impl BikeRentalIntegrationEventRoute {
    pub const fn address(&self) -> &IntegrationEventAddress {
        &self.address
    }
    pub const fn consumer(&self) -> &ConsumerConfig<IntegrationEventAddress> {
        &self.consumer
    }
}

/// Released resource identities remain explicit policy, including the legacy
/// command subscription generation 2 (independent of command schema versions).
#[derive(Clone)]
pub struct BikeRentalNatsConfig {
    runtime: NatsRuntimeConfig,
    resource_limits: BikeRentalNatsResourceLimits,
    command_routes: [BikeRentalCommandRoute; 4],
    integration_event_route: BikeRentalIntegrationEventRoute,
}
impl BikeRentalNatsConfig {
    pub fn new(application: &str) -> Result<Self, BikeRentalNatsError> {
        Self::new_with_resource_limits(application, BikeRentalNatsResourceLimits::default())
    }
    pub fn new_test(application: &str) -> Result<Self, BikeRentalNatsError> {
        Self::new_test_with_resource_limits(application, BikeRentalNatsResourceLimits::default())
    }
    pub fn new_with_resource_limits(
        application: &str,
        limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::new_in_scope(application, TrafficScope::Normal, limits)
    }
    pub fn new_test_with_resource_limits(
        application: &str,
        limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::new_in_scope(application, TrafficScope::Test, limits)
    }
    fn new_in_scope(
        application: &str,
        scope: TrafficScope,
        limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::for_bindings(
            application,
            scope,
            limits,
            &crate::tracer::command_bindings()?,
        )
    }

    pub fn for_bindings(
        application: &str,
        scope: TrafficScope,
        limits: BikeRentalNatsResourceLimits,
        bindings: &rostfrei_tracer::ApplicationBindings,
    ) -> Result<Self, BikeRentalNatsError> {
        let context = ApplicationName::new(application)?
            .bounded_context_in_scope(scope, BOUNDED_CONTEXT_NAME)?;
        let mut runtime =
            NatsRuntimeConfig::new(context.clone(), bindings, COMMAND_SUBSCRIPTION_GENERATION)?;
        runtime.messaging = ApplicationMessagingConfig::new_in_scope(context.application(), scope)?
            .with_max_bytes(limits.messaging_stream_max_bytes())?;
        runtime.event_store = NatsEventStoreConfig::for_bounded_context(&context)?
            .with_storage_limits(
                limits.event_store_max_stream_bytes(),
                limits.event_store_max_event_bytes(),
            )?;
        runtime.domain_events = NatsDomainEventConsumerConfig::new(
            context.consumer_name("bicycle-rental-started-publisher", 1)?,
            context.durable_name("bicycle-rental-started-publisher", 1)?,
            Duration::from_secs(45),
            Duration::from_secs(30),
            RetryDelay::new(Duration::from_secs(1))?,
        )?;
        let command_routes = [
            command_route::<RentBicycle>(&context, BikeRentalCommand::RentBicycle)?,
            command_route::<ReturnBicycle>(&context, BikeRentalCommand::ReturnBicycle)?,
            command_route::<AddBicycle>(&context, BikeRentalCommand::AddBicycle)?,
            command_route::<TransferBicycle>(&context, BikeRentalCommand::TransferBicycle)?,
        ];
        let address = context.integration_event_address(BICYCLE_RENTAL_STARTED_EVENT_NAME)?;
        let consumer = runtime
            .integrations
            .first()
            .ok_or_else(|| {
                BikeRentalNatsError::Configuration("missing rental integration consumer".into())
            })?
            .clone();
        Ok(Self {
            runtime,
            resource_limits: limits,
            command_routes,
            integration_event_route: BikeRentalIntegrationEventRoute { address, consumer },
        })
    }
    pub const fn host_config(&self) -> &NatsRuntimeConfig {
        &self.runtime
    }
    pub const fn application(&self) -> &ApplicationName {
        self.runtime.context.application()
    }
    pub const fn context(&self) -> &BoundedContext {
        &self.runtime.context
    }
    pub const fn resource_limits(&self) -> BikeRentalNatsResourceLimits {
        self.resource_limits
    }
    pub const fn messaging(&self) -> &ApplicationMessagingConfig {
        &self.runtime.messaging
    }
    pub const fn event_store(&self) -> &NatsEventStoreConfig {
        &self.runtime.event_store
    }
    pub const fn domain_event_consumer(&self) -> &NatsDomainEventConsumerConfig {
        &self.runtime.domain_events
    }
    pub const fn command_routes(&self) -> &[BikeRentalCommandRoute; 4] {
        &self.command_routes
    }
    pub const fn command_route(&self, command: BikeRentalCommand) -> &BikeRentalCommandRoute {
        let [rent, returned, added, transferred] = &self.command_routes;
        match command {
            BikeRentalCommand::RentBicycle => rent,
            BikeRentalCommand::ReturnBicycle => returned,
            BikeRentalCommand::AddBicycle => added,
            BikeRentalCommand::TransferBicycle => transferred,
        }
    }
    pub const fn integration_event_route(&self) -> &BikeRentalIntegrationEventRoute {
        &self.integration_event_route
    }
    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), BikeRentalNatsError> {
        Box::pin(self.runtime.provision(connection)).await?;
        rostfrei_nats::provision_domain_event_consumer(
            connection.jetstream(),
            &self.runtime.event_store,
            &self.runtime.domain_events,
        )
        .await?;
        Ok(())
    }
    pub async fn connect_store(
        &self,
        connection: &NatsConnection,
    ) -> Result<NatsEventStore, BikeRentalNatsError> {
        Ok(NatsEventStore::connect(
            connection.jetstream().clone(),
            self.runtime.event_store.clone(),
        )
        .await?)
    }
}
fn command_route<C: Command>(
    context: &BoundedContext,
    command: BikeRentalCommand,
) -> Result<BikeRentalCommandRoute, ContractError> {
    let address = context.command_address(C::LOCAL_ID)?;
    let consumer = ConsumerConfig::new(
        context.consumer_name(C::LOCAL_ID, COMMAND_SUBSCRIPTION_GENERATION)?,
        context.durable_name(C::LOCAL_ID, COMMAND_SUBSCRIPTION_GENERATION)?,
        address.clone(),
        Duration::from_secs(45),
        Duration::from_secs(30),
        1,
        5,
    )?;
    Ok(BikeRentalCommandRoute {
        command,
        address,
        consumer,
    })
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_field_names)]
pub struct BicycleRentalStarted {
    source_event_id: String,
    fleet_id: FleetId,
    bicycle_id: BicycleId,
}
impl BicycleRentalStarted {
    pub fn source_event_id(&self) -> &str {
        &self.source_event_id
    }
    pub const fn fleet_id(&self) -> &FleetId {
        &self.fleet_id
    }
    pub const fn bicycle_id(&self) -> &BicycleId {
        &self.bicycle_id
    }
}
impl IntegrationEvent for BicycleRentalStarted {
    const EVENT_NAME: &'static str = BICYCLE_RENTAL_STARTED_EVENT_NAME;
    const SCHEMA_VERSION: u32 = 1;
}
pub struct BicycleRentedIntegrationMapper;
impl IntegrationEventMapper<BicycleRented> for BicycleRentedIntegrationMapper {
    type Output = BicycleRentalStarted;
    fn map(&self, event: &CommittedDomainEvent<'_, BicycleRented>) -> Self::Output {
        BicycleRentalStarted {
            source_event_id: event.recorded().event_id().as_str().to_owned(),
            fleet_id: event.event().fleet_id.clone(),
            bicycle_id: event.event().bicycle_id.clone(),
        }
    }
}
pub struct BicycleRentalStartedHandler;
#[async_trait]
impl MessageHandler<IntegrationEventAddress> for BicycleRentalStartedHandler {
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        let envelope = EncodedIntegrationMessage::from_delivery(
            delivery.address().clone(),
            delivery.message_id().clone(),
            delivery.payload().to_vec(),
            delivery.correlation_id().cloned(),
        )
        .and_then(|message| message.decode::<BicycleRentalStarted>());
        let Ok(envelope) = envelope else {
            return QuarantineReason::new("invalid bicycle-rental-started envelope").map_or(
                DeliveryDisposition::Terminate,
                DeliveryDisposition::Quarantine,
            );
        };
        tracing::info!(message_id = %envelope.message_id(), correlation_id = %envelope.correlation_id(), source_event_id = envelope.payload().source_event_id(), fleet_id = envelope.payload().fleet_id().as_str(), bicycle_id = envelope.payload().bicycle_id().as_str(), "bicycle rental integration event consumed");
        DeliveryDisposition::Acknowledge
    }
}

/// Compatibility facade for the example's inspection and integration tests.
/// All runtime lifecycle and Tracer adapters are supplied by the framework.
pub struct BikeRentalNatsRuntime {
    config: BikeRentalNatsConfig,
    inner: Arc<NatsRuntime>,
}
impl BikeRentalNatsRuntime {
    pub async fn provision(
        connection: NatsConnection,
        application: &str,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::provision_with_resource_limits(
            connection,
            application,
            BikeRentalNatsResourceLimits::default(),
        )
        .await
    }
    pub async fn provision_test(
        connection: NatsConnection,
        application: &str,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::provision_test_with_resource_limits(
            connection,
            application,
            BikeRentalNatsResourceLimits::default(),
        )
        .await
    }
    pub async fn provision_with_resource_limits(
        connection: NatsConnection,
        application: &str,
        limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::from_config(
            connection,
            BikeRentalNatsConfig::new_with_resource_limits(application, limits)?,
        )
        .await
    }
    pub async fn provision_test_with_resource_limits(
        connection: NatsConnection,
        application: &str,
        limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::from_config(
            connection,
            BikeRentalNatsConfig::new_test_with_resource_limits(application, limits)?,
        )
        .await
    }
    async fn from_config(
        connection: NatsConnection,
        config: BikeRentalNatsConfig,
    ) -> Result<Self, BikeRentalNatsError> {
        let inner = Arc::new(
            Box::pin(NatsRuntime::provision(
                connection,
                config.runtime.clone(),
                Arc::new(crate::tracer::command_bindings()?),
            ))
            .await?,
        );
        Ok(Self { config, inner })
    }
    pub const fn config(&self) -> &BikeRentalNatsConfig {
        &self.config
    }
    pub fn store(&self) -> &NatsEventStore {
        self.inner.store()
    }
    pub fn transport(&self) -> Arc<dyn CommandTransport> {
        self.inner.transport()
    }
    pub fn quarantine_reader(&self) -> Arc<dyn rostfrei_messaging_core::QuarantineReader> {
        self.inner.quarantine_reader()
    }
    pub async fn apply_fixture(
        &self,
        fixture: &Fixture,
    ) -> Result<FixtureApplyReport, BikeRentalNatsError> {
        self.inner.apply_fixture(fixture).await
    }
    pub async fn start_workers(&self) -> Result<(), BikeRentalNatsError> {
        self.inner.start_workers().await
    }
    pub async fn stop_workers(&self) {
        self.inner.stop_workers().await;
    }
    pub async fn wait_for_worker_exit(&self) {
        let _ = self.inner.wait_for_worker_exit().await;
    }
    pub async fn start_correlation_observer(
        &self,
        observer: CorrelationObserver,
    ) -> Result<tokio::task::JoinHandle<Result<(), BikeRentalNatsError>>, BikeRentalNatsError> {
        self.inner.start_correlation_observer(observer).await
    }
}
#[async_trait]
impl TestScenarioReset for BikeRentalNatsRuntime {
    async fn reset(&self, fixture: &Fixture) -> Result<(), TestScenarioResetError> {
        self.inner.reset(fixture).await
    }
}

#[cfg(test)]
fn integration_event_observation(
    subject: &str,
    message_id: Option<&rostfrei::MessageId>,
    correlation_id: &rostfrei::CorrelationId,
    payload: &[u8],
) -> Result<rostfrei_tracer::IntegrationEventObservation, String> {
    let address = subject
        .parse::<IntegrationEventAddress>()
        .map_err(|e| e.to_string())?;
    let message_id = message_id.cloned().ok_or("missing identity")?;
    let message = EncodedIntegrationMessage::from_delivery(
        address,
        message_id,
        payload.to_vec(),
        Some(correlation_id.clone()),
    )
    .map_err(|e| e.to_string())?;
    let envelope = message
        .decode::<BicycleRentalStarted>()
        .map_err(|e| e.to_string())?;
    Ok(rostfrei_tracer::IntegrationEventObservation::new(
        envelope.message_id().as_str(),
        BicycleRentalStarted::EVENT_NAME,
        envelope.schema_version().get(),
        subject,
    )
    .with_causation_id(envelope.causation_id().ok_or("missing causation")?.as_str())
    .with_payload(serde_json::to_value(envelope.payload()).map_err(|e| e.to_string())?))
}
