use std::{env, sync::Arc, time::Duration};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rostfrei::{
    CommandBindingRegistrationError, CommandBus, CommandDefinition, CommandMessageAdapter,
    CommandProcessor, CommittedDomainEvent, CommittedEventContext, DomainEventDefinitionType,
    DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRegistrationError, EncodedIntegrationMessage,
    EventStore, EventStoreError, InfallibleCommandRejectionMapper, IntegrationEvent,
    IntegrationEventBus, IntegrationEventBusError, IntegrationEventBusErrorKind,
    IntegrationMessageAdapter, JsonDomainRejectionMapper,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CommandAddress, CommandRejectionClassification, ConsumeError,
    ConsumerConfig, ContractError, DeliveryDisposition, IntegrationEventAddress, MessageDelivery,
    MessageHandler, QuarantineReason, RetryDelay, TrafficScope,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, CorrelatedMessage, CorrelatedMessageFamily,
    CorrelatedMessageHandler, DEFAULT_EVENT_STORE_MAX_EVENT_BYTES,
    DEFAULT_EVENT_STORE_MAX_STREAM_BYTES, DomainEventConsumerError, NatsConnection,
    NatsCorrelationObserver, NatsDomainEventConsumer, NatsDomainEventConsumerConfig, NatsError,
    NatsEventStore, NatsEventStoreConfig, NatsMessagingAdapter, provision_application_messaging,
    provision_domain_event_consumer, provision_durable_consumer, provision_event_store,
};
use rostfrei_tracer::{
    CommandBusTransport, CommandInvocation, CommandTransport, CommandTransportError,
    CommandTransportObserver, CorrelationError, CorrelationObserver, DomainEventObservation,
    FIXTURE_OPERATION_ID_PREFIX, IntegrationEventObservation, MaterializedTestFixture,
    TestScenarioReset, TestScenarioResetError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock, watch},
    task::JoinHandle,
};

use crate::{
    demo::{SeedError, materialize_fixture, seed_demo},
    rental_fleet::{
        AddBicycle, BicycleId, BicycleRented, FleetId, RentBicycle, RentalFleetAggregate,
        ReturnBicycle,
    },
};

#[cfg(test)]
mod tests;

pub const BOUNDED_CONTEXT_NAME: &str = "bike-rental";
pub const APPLICATION_NAME: &str = "bike-rental";
pub const BICYCLE_RENTAL_STARTED_EVENT_NAME: &str = "bicycle-rental-started";
const BICYCLE_RENTAL_STARTED_SCHEMA_VERSION: u32 = 1;
const DOMAIN_EVENT_PUBLISHER_PURPOSE: &str = "bicycle-rental-started-publisher";
const INTEGRATION_EVENT_CONSUMER_PURPOSE: &str = "bicycle-rental-started-consumer";
const RETRY_DELAY: Duration = Duration::from_secs(1);
const CONSUMER_ACK_WAIT: Duration = Duration::from_secs(45);
const CONSUMER_PROCESSING_TIMEOUT: Duration = Duration::from_secs(30);
const CONSUMER_CONCURRENCY: usize = 4;
const COMMAND_CONSUMER_CONCURRENCY: usize = 1;
const MAXIMUM_DELIVERY_ATTEMPTS: u32 = 5;
const COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MESSAGING_STREAM_MAX_BYTES: i64 = 64 * 1024 * 1024;
const MESSAGING_STREAM_MAX_BYTES_ENV: &str = "ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES";
const EVENT_STORE_MAX_STREAM_BYTES_ENV: &str = "ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES";
const EVENT_STORE_MAX_EVENT_BYTES_ENV: &str = "ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES";

#[derive(Debug, Error)]
pub enum BikeRentalNatsError {
    #[error(transparent)]
    CommandBinding(#[from] CommandBindingRegistrationError),
    #[error(transparent)]
    Consume(#[from] ConsumeError),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    DomainEventConsumer(#[from] DomainEventConsumerError),
    #[error(transparent)]
    DomainEventRegistration(#[from] DomainEventRegistrationError),
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
    #[error(transparent)]
    Nats(#[from] NatsError),
    #[error("environment variable `{name}` must be a positive integer byte count, got `{value}`")]
    ResourceLimitEnvironment { name: &'static str, value: String },
    #[error("only a test-scoped NATS runtime can be reset")]
    ResetRequiresTestScope,
    #[error(transparent)]
    Seed(#[from] SeedError),
}

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
            env_i64(MESSAGING_STREAM_MAX_BYTES_ENV, defaults.messaging_stream)?,
            env_i64(
                EVENT_STORE_MAX_STREAM_BYTES_ENV,
                defaults.event_store_stream,
            )?,
            env_usize(EVENT_STORE_MAX_EVENT_BYTES_ENV, defaults.event_store_event)?,
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
            DEFAULT_MESSAGING_STREAM_MAX_BYTES,
            DEFAULT_EVENT_STORE_MAX_STREAM_BYTES,
            DEFAULT_EVENT_STORE_MAX_EVENT_BYTES,
        )
    }
}

fn env_i64(name: &'static str, default: i64) -> Result<i64, BikeRentalNatsError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or(BikeRentalNatsError::ResourceLimitEnvironment { name, value }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(BikeRentalNatsError::ResourceLimitEnvironment {
            name,
            value: "<non-Unicode>".to_owned(),
        }),
    }
}

fn env_usize(name: &'static str, default: usize) -> Result<usize, BikeRentalNatsError> {
    match env::var(name) {
        Ok(value) => value
            .parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or(BikeRentalNatsError::ResourceLimitEnvironment { name, value }),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(env::VarError::NotUnicode(_)) => Err(BikeRentalNatsError::ResourceLimitEnvironment {
            name,
            value: "<non-Unicode>".to_owned(),
        }),
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BikeRentalCommand {
    RentBicycle,
    ReturnBicycle,
    AddBicycle,
}

impl BikeRentalCommand {
    pub const fn command_name(self) -> &'static str {
        match self {
            Self::RentBicycle => RentBicycle::COMMAND_NAME,
            Self::ReturnBicycle => ReturnBicycle::COMMAND_NAME,
            Self::AddBicycle => AddBicycle::COMMAND_NAME,
        }
    }

    pub const fn schema_version(self) -> u32 {
        match self {
            Self::RentBicycle => <RentBicycle as CommandDefinition>::SCHEMA_VERSION,
            Self::ReturnBicycle => <ReturnBicycle as CommandDefinition>::SCHEMA_VERSION,
            Self::AddBicycle => <AddBicycle as CommandDefinition>::SCHEMA_VERSION,
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

#[derive(Clone)]
pub struct BikeRentalNatsConfig {
    application: ApplicationName,
    context: BoundedContext,
    resource_limits: BikeRentalNatsResourceLimits,
    messaging: ApplicationMessagingConfig,
    event_store: NatsEventStoreConfig,
    domain_event_consumer: NatsDomainEventConsumerConfig,
    command_routes: [BikeRentalCommandRoute; 3],
    integration_event_route: BikeRentalIntegrationEventRoute,
}

impl BikeRentalNatsConfig {
    pub fn new(application: &str) -> Result<Self, BikeRentalNatsError> {
        Self::new_in_scope(
            application,
            TrafficScope::Normal,
            BikeRentalNatsResourceLimits::default(),
        )
    }

    pub fn new_test(application: &str) -> Result<Self, BikeRentalNatsError> {
        Self::new_in_scope(
            application,
            TrafficScope::Test,
            BikeRentalNatsResourceLimits::default(),
        )
    }

    pub fn new_with_resource_limits(
        application: &str,
        resource_limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::new_in_scope(application, TrafficScope::Normal, resource_limits)
    }

    pub fn new_test_with_resource_limits(
        application: &str,
        resource_limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::new_in_scope(application, TrafficScope::Test, resource_limits)
    }

    fn new_in_scope(
        application: &str,
        traffic_scope: TrafficScope,
        resource_limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        let application = ApplicationName::new(application)?;
        let context = application.bounded_context_in_scope(traffic_scope, BOUNDED_CONTEXT_NAME)?;
        let messaging = ApplicationMessagingConfig::new_in_scope(&application, traffic_scope)?
            .with_max_bytes(resource_limits.messaging_stream_max_bytes())?;
        let event_store = NatsEventStoreConfig::for_bounded_context(&context)?
            .with_storage_limits(
                resource_limits.event_store_max_stream_bytes(),
                resource_limits.event_store_max_event_bytes(),
            )?;
        let domain_event_consumer = NatsDomainEventConsumerConfig::new(
            context.consumer_name(DOMAIN_EVENT_PUBLISHER_PURPOSE, 1)?,
            context.durable_name(DOMAIN_EVENT_PUBLISHER_PURPOSE, 1)?,
            CONSUMER_ACK_WAIT,
            CONSUMER_PROCESSING_TIMEOUT,
            RetryDelay::new(RETRY_DELAY)?,
        )?;
        let command_routes = [
            command_route(&context, BikeRentalCommand::RentBicycle)?,
            command_route(&context, BikeRentalCommand::ReturnBicycle)?,
            command_route(&context, BikeRentalCommand::AddBicycle)?,
        ];
        let integration_event_route = integration_event_route(&context)?;
        Ok(Self {
            application,
            context,
            resource_limits,
            messaging,
            event_store,
            domain_event_consumer,
            command_routes,
            integration_event_route,
        })
    }

    pub const fn application(&self) -> &ApplicationName {
        &self.application
    }

    pub const fn context(&self) -> &BoundedContext {
        &self.context
    }

    pub const fn resource_limits(&self) -> BikeRentalNatsResourceLimits {
        self.resource_limits
    }

    pub const fn messaging(&self) -> &ApplicationMessagingConfig {
        &self.messaging
    }

    pub const fn event_store(&self) -> &NatsEventStoreConfig {
        &self.event_store
    }

    pub const fn domain_event_consumer(&self) -> &NatsDomainEventConsumerConfig {
        &self.domain_event_consumer
    }

    pub const fn command_routes(&self) -> &[BikeRentalCommandRoute; 3] {
        &self.command_routes
    }

    pub const fn command_route(&self, command: BikeRentalCommand) -> &BikeRentalCommandRoute {
        let [rent, returned, added] = &self.command_routes;
        match command {
            BikeRentalCommand::RentBicycle => rent,
            BikeRentalCommand::ReturnBicycle => returned,
            BikeRentalCommand::AddBicycle => added,
        }
    }

    pub const fn integration_event_route(&self) -> &BikeRentalIntegrationEventRoute {
        &self.integration_event_route
    }

    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), BikeRentalNatsError> {
        provision_application_messaging(connection.jetstream(), &self.messaging).await?;
        provision_event_store(connection.jetstream(), &self.event_store).await?;
        provision_domain_event_consumer(
            connection.jetstream(),
            &self.event_store,
            &self.domain_event_consumer,
        )
        .await?;
        for route in &self.command_routes {
            provision_durable_consumer(
                connection.jetstream(),
                self.messaging.topology(),
                &route.consumer,
            )
            .await?;
        }
        provision_durable_consumer(
            connection.jetstream(),
            self.messaging.topology(),
            &self.integration_event_route.consumer,
        )
        .await?;
        Ok(())
    }

    pub async fn connect_store(
        &self,
        connection: &NatsConnection,
    ) -> Result<NatsEventStore, BikeRentalNatsError> {
        NatsEventStore::connect(connection.jetstream().clone(), self.event_store.clone())
            .await
            .map_err(Into::into)
    }
}

fn command_route(
    context: &BoundedContext,
    command: BikeRentalCommand,
) -> Result<BikeRentalCommandRoute, ContractError> {
    let name = command.command_name();
    let schema_version = command.schema_version();
    let address = context.command_address(name)?;
    let consumer = ConsumerConfig::new(
        context.consumer_name(name, schema_version)?,
        context.durable_name(name, schema_version)?,
        address.clone(),
        CONSUMER_ACK_WAIT,
        CONSUMER_PROCESSING_TIMEOUT,
        COMMAND_CONSUMER_CONCURRENCY,
        MAXIMUM_DELIVERY_ATTEMPTS,
    )?;
    Ok(BikeRentalCommandRoute {
        command,
        address,
        consumer,
    })
}

fn integration_event_route(
    context: &BoundedContext,
) -> Result<BikeRentalIntegrationEventRoute, ContractError> {
    let address = context.integration_event_address(BICYCLE_RENTAL_STARTED_EVENT_NAME)?;
    let consumer = ConsumerConfig::new(
        context.consumer_name(INTEGRATION_EVENT_CONSUMER_PURPOSE, 1)?,
        context.durable_name(INTEGRATION_EVENT_CONSUMER_PURPOSE, 1)?,
        address.clone(),
        CONSUMER_ACK_WAIT,
        CONSUMER_PROCESSING_TIMEOUT,
        CONSUMER_CONCURRENCY,
        MAXIMUM_DELIVERY_ATTEMPTS,
    )?;
    Ok(BikeRentalIntegrationEventRoute { address, consumer })
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
    const SCHEMA_VERSION: u32 = BICYCLE_RENTAL_STARTED_SCHEMA_VERSION;
}

pub struct BicycleRentedIntegrationMapper {
    bus: IntegrationEventBus,
}

impl BicycleRentedIntegrationMapper {
    pub const fn new(bus: IntegrationEventBus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl DomainEventHandler<BicycleRented> for BicycleRentedIntegrationMapper {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, BicycleRented>,
    ) -> Result<(), DomainEventHandlerError> {
        if event
            .recorded()
            .operation_id()
            .as_str()
            .starts_with(FIXTURE_OPERATION_ID_PREFIX)
        {
            return Ok(());
        }
        let committed = CommittedEventContext::new(event.recorded())
            .map_err(|error| classify_integration_event_error(&error))?;
        self.bus
            .publish(
                committed,
                BicycleRentalStarted {
                    source_event_id: event.recorded().event_id().as_str().to_owned(),
                    fleet_id: event.event().fleet_id.clone(),
                    bicycle_id: event.event().bicycle_id.clone(),
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| classify_integration_event_error(&error))
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
        tracing::info!(
            message_id = %envelope.message_id(),
            correlation_id = %envelope.correlation_id(),
            source_event_id = envelope.payload().source_event_id(),
            fleet_id = envelope.payload().fleet_id().as_str(),
            bicycle_id = envelope.payload().bicycle_id().as_str(),
            "bicycle rental integration event consumed"
        );
        DeliveryDisposition::Acknowledge
    }
}

fn classify_integration_event_error(error: &IntegrationEventBusError) -> DomainEventHandlerError {
    let kind = match error.kind() {
        IntegrationEventBusErrorKind::InvalidContext => {
            DomainEventHandlerErrorKind::InvalidCommittedEvent
        }
        IntegrationEventBusErrorKind::Timeout | IntegrationEventBusErrorKind::Unavailable => {
            DomainEventHandlerErrorKind::Retryable
        }
        _ => DomainEventHandlerErrorKind::OperatorBlocking,
    };
    DomainEventHandlerError::new(kind, error.to_string())
}

struct ScopedCommandTransport {
    inner: Arc<dyn CommandTransport>,
    scope_gate: Arc<RwLock<()>>,
}

impl ScopedCommandTransport {
    const fn new(inner: Arc<dyn CommandTransport>, scope_gate: Arc<RwLock<()>>) -> Self {
        Self { inner, scope_gate }
    }
}

#[async_trait]
impl CommandTransport for ScopedCommandTransport {
    fn maximum_payload_len(&self) -> usize {
        self.inner.maximum_payload_len()
    }

    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<rostfrei_tracer::CommandReceipt, CommandTransportError> {
        let _scope = Arc::clone(&self.scope_gate).read_owned().await;
        self.inner.invoke(invocation, observer).await
    }
}

pub struct BikeRentalNatsRuntime {
    connection: NatsConnection,
    config: BikeRentalNatsConfig,
    store: NatsEventStore,
    messaging: Arc<NatsMessagingAdapter>,
    transport: Arc<ScopedCommandTransport>,
    scope_gate: Arc<RwLock<()>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

struct TracerCorrelationHandler {
    observer: CorrelationObserver,
}

#[async_trait]
impl CorrelatedMessageHandler for TracerCorrelationHandler {
    async fn handle(&self, message: CorrelatedMessage) {
        let result = match message.family() {
            CorrelatedMessageFamily::DomainEvent => {
                observe_domain_message(&self.observer, &message).await
            }
            CorrelatedMessageFamily::IntegrationEvent => {
                observe_integration_message(&self.observer, &message).await
            }
        };
        if let Err(error) = result
            && !matches!(error, CorrelationError::NotFound)
        {
            tracing::warn!(%error, "correlated NATS message could not be observed");
        }
    }
}

impl BikeRentalNatsRuntime {
    pub async fn provision(
        connection: NatsConnection,
        application: &str,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::provision_in_scope(
            connection,
            application,
            TrafficScope::Normal,
            BikeRentalNatsResourceLimits::default(),
        )
        .await
    }

    pub async fn provision_test(
        connection: NatsConnection,
        application: &str,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::provision_in_scope(
            connection,
            application,
            TrafficScope::Test,
            BikeRentalNatsResourceLimits::default(),
        )
        .await
    }

    pub async fn provision_with_resource_limits(
        connection: NatsConnection,
        application: &str,
        resource_limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::provision_in_scope(
            connection,
            application,
            TrafficScope::Normal,
            resource_limits,
        )
        .await
    }

    pub async fn provision_test_with_resource_limits(
        connection: NatsConnection,
        application: &str,
        resource_limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        Self::provision_in_scope(connection, application, TrafficScope::Test, resource_limits).await
    }

    async fn provision_in_scope(
        connection: NatsConnection,
        application: &str,
        traffic_scope: TrafficScope,
        resource_limits: BikeRentalNatsResourceLimits,
    ) -> Result<Self, BikeRentalNatsError> {
        let config = match traffic_scope {
            TrafficScope::Normal => {
                BikeRentalNatsConfig::new_with_resource_limits(application, resource_limits)?
            }
            TrafficScope::Test => {
                BikeRentalNatsConfig::new_test_with_resource_limits(application, resource_limits)?
            }
        };
        config.provision(&connection).await?;
        let store = config.connect_store(&connection).await?;
        let messaging = Arc::new(
            connection
                .messaging_adapter(config.messaging.topology().clone())
                .with_response_timeout(COMMAND_RESPONSE_TIMEOUT),
        );
        let command_adapter: Arc<dyn CommandMessageAdapter> = messaging.clone();
        let command_bus = CommandBus::new(config.context.clone(), command_adapter);
        let bus_transport: Arc<dyn CommandTransport> =
            Arc::new(CommandBusTransport::new(command_bus));
        let scope_gate = Arc::new(RwLock::new(()));
        let transport = Arc::new(ScopedCommandTransport::new(
            bus_transport,
            Arc::clone(&scope_gate),
        ));
        Ok(Self {
            connection,
            config,
            store,
            messaging,
            transport,
            scope_gate,
            workers: Mutex::new(Vec::new()),
        })
    }

    pub const fn config(&self) -> &BikeRentalNatsConfig {
        &self.config
    }

    pub const fn store(&self) -> &NatsEventStore {
        &self.store
    }

    pub fn transport(&self) -> Arc<dyn CommandTransport> {
        self.transport.clone()
    }

    pub async fn seed_demo(&self) -> Result<(), BikeRentalNatsError> {
        seed_demo(&self.store).await.map_err(Into::into)
    }

    pub async fn start_workers(&self) -> Result<(), BikeRentalNatsError> {
        let _scope = Arc::clone(&self.scope_gate).write_owned().await;
        self.start_workers_in_scope().await
    }

    pub async fn wait_for_worker_exit(&self) {
        loop {
            if self
                .workers
                .lock()
                .await
                .iter()
                .any(JoinHandle::is_finished)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn command_processor(&self) -> Result<Arc<CommandProcessor>, BikeRentalNatsError> {
        let event_store: Arc<dyn EventStore> = Arc::new(self.store.clone());
        let mut processor = CommandProcessor::new(event_store);
        processor.register::<RentBicycle, _>(JsonDomainRejectionMapper::new(
            CommandRejectionClassification::Conflict,
        ))?;
        processor.register::<ReturnBicycle, _>(JsonDomainRejectionMapper::new(
            CommandRejectionClassification::Conflict,
        ))?;
        processor.register::<AddBicycle, _>(InfallibleCommandRejectionMapper)?;
        Ok(Arc::new(processor))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "worker preparation is kept together so startup remains atomic"
    )]
    async fn start_workers_in_scope(&self) -> Result<(), BikeRentalNatsError> {
        let mut workers = self.workers.lock().await;
        if !workers.is_empty() {
            if workers.iter().all(|worker| !worker.is_finished()) {
                return Ok(());
            }
            let stale = std::mem::take(&mut *workers);
            for worker in &stale {
                worker.abort();
            }
            for worker in stale {
                let _ = worker.await;
            }
        }

        let factory = self
            .connection
            .consumer_factory(self.config.messaging.topology().clone());
        let command_handler: Arc<dyn MessageHandler<CommandAddress>> =
            Arc::new(self.messaging.command_handler(self.command_processor()?));
        let mut command_consumers = Vec::with_capacity(self.config.command_routes.len());
        for route in &self.config.command_routes {
            factory.verify_consumer(&route.consumer).await?;
            let consumer = rostfrei_messaging_core::MessageConsumerFactory::create(
                &factory,
                route.consumer.clone(),
            )?;
            command_consumers.push((route.command, consumer));
        }

        let integration_route = self.config.integration_event_route();
        factory.verify_consumer(&integration_route.consumer).await?;
        let integration_consumer = rostfrei_messaging_core::MessageConsumerFactory::create(
            &factory,
            integration_route.consumer.clone(),
        )?;
        let integration_handler: Arc<dyn MessageHandler<IntegrationEventAddress>> =
            Arc::new(BicycleRentalStartedHandler);

        let integration_adapter: Arc<dyn IntegrationMessageAdapter> = self.messaging.clone();
        let integration_bus =
            IntegrationEventBus::new(self.config.context.clone(), integration_adapter);
        let mut dispatcher = DomainEventDispatcher::new();
        dispatcher.register::<RentalFleetAggregate, BicycleRented, _>(
            BicycleRented::DEFINITION.id,
            Arc::new(BicycleRentedIntegrationMapper::new(integration_bus)),
        )?;
        let domain_consumer = NatsDomainEventConsumer::connect(
            self.connection.jetstream().clone(),
            self.config.event_store.clone(),
            self.config.domain_event_consumer.clone(),
            Arc::new(dispatcher),
        )
        .await?;

        let (domain_shutdown, domain_shutdown_receiver) = watch::channel(false);
        let domain_durable = self
            .config
            .domain_event_consumer
            .durable_name()
            .as_str()
            .to_owned();
        workers.push(tokio::spawn(async move {
            let _shutdown_keepalive = domain_shutdown;
            if let Err(error) = domain_consumer
                .run_until_shutdown(domain_shutdown_receiver)
                .await
            {
                tracing::error!(
                    durable = domain_durable,
                    %error,
                    "bike-rental domain-event worker stopped"
                );
            }
        }));
        workers.push(tokio::spawn(async move {
            if let Err(error) = integration_consumer.run(integration_handler).await {
                tracing::error!(
                    event = BICYCLE_RENTAL_STARTED_EVENT_NAME,
                    %error,
                    "bike-rental integration-event worker stopped"
                );
            }
        }));
        for (command, consumer) in command_consumers {
            let handler = Arc::clone(&command_handler);
            workers.push(tokio::spawn(async move {
                if let Err(error) = consumer.run(handler).await {
                    tracing::error!(
                        command = command.command_name(),
                        %error,
                        "bike-rental command worker stopped"
                    );
                }
            }));
        }
        drop(workers);
        Ok(())
    }

    pub async fn start_correlation_observer(
        &self,
        observer: CorrelationObserver,
    ) -> Result<JoinHandle<()>, BikeRentalNatsError> {
        let nats_observer = NatsCorrelationObserver::new_in_scope(
            self.connection.client().clone(),
            self.config.application().clone(),
            self.config.context().traffic_scope(),
        )
        .with_streams(
            self.config.event_store().stream_name(),
            self.config
                .messaging()
                .topology()
                .integration_event_stream()
                .as_str(),
        );
        let subscription = nats_observer.subscribe().await?;
        Ok(tokio::spawn(async move {
            if let Err(error) = subscription
                .run(Arc::new(TracerCorrelationHandler { observer }))
                .await
            {
                tracing::error!(%error, "bike-rental correlation observer stopped");
            }
        }))
    }

    pub async fn stop_workers(&self) {
        let _scope = Arc::clone(&self.scope_gate).write_owned().await;
        self.stop_workers_in_scope().await;
    }

    async fn stop_workers_in_scope(&self) {
        let workers = {
            let mut workers = self.workers.lock().await;
            std::mem::take(&mut *workers)
        };
        for worker in &workers {
            worker.abort();
        }
        for worker in workers {
            let _ = worker.await;
        }
    }

    async fn delete_resources(&self) -> Result<(), BikeRentalNatsError> {
        let topology = self.config.messaging.topology();
        let streams = [
            topology.command_stream().as_str(),
            topology.command_response_stream().as_str(),
            topology.integration_event_stream().as_str(),
            topology.quarantine_stream().as_str(),
            self.config.event_store.stream_name(),
        ];
        for stream in streams {
            self.connection.delete_stream_if_exists(stream).await?;
        }
        Ok(())
    }

    async fn materialize_fixture(
        &self,
        fixture: &MaterializedTestFixture,
    ) -> Result<(), BikeRentalNatsError> {
        materialize_fixture(&self.store, fixture)
            .await
            .map_err(Into::into)
    }

    async fn reset_scope(
        &self,
        fixture: &MaterializedTestFixture,
    ) -> Result<(), BikeRentalNatsError> {
        if self.config.context().traffic_scope() != TrafficScope::Test {
            return Err(BikeRentalNatsError::ResetRequiresTestScope);
        }
        let _scope = Arc::clone(&self.scope_gate).write_owned().await;
        self.stop_workers_in_scope().await;
        self.delete_resources().await?;
        self.config.provision(&self.connection).await?;
        self.materialize_fixture(fixture).await?;
        self.start_workers_in_scope().await
    }
}

async fn observe_domain_message(
    observer: &CorrelationObserver,
    message: &CorrelatedMessage,
) -> Result<(), CorrelationError> {
    let Ok(wire) = serde_json::from_slice::<Value>(message.payload()) else {
        return Ok(());
    };
    let Some(event) = wire.get("event") else {
        return Ok(());
    };
    let Some(event_type) = event.get("eventType").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(schema_version) = event
        .get("eventSchemaVersion")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    else {
        return Ok(());
    };
    let mut observation = DomainEventObservation::new(event_type, schema_version);
    if let Some(message_id) = event.get("eventId").and_then(Value::as_str) {
        observation = observation.with_message_id(message_id);
    }
    if let Some(causation_id) = event.get("causationId").and_then(Value::as_str) {
        observation = observation.with_causation_id(causation_id);
    }
    if let Some(stream_version) = event.get("streamVersion").and_then(Value::as_u64) {
        observation = observation.with_stream_version(stream_version);
    }
    if let Some(encoded) = event.get("payloadBase64").and_then(Value::as_str)
        && let Ok(payload) = STANDARD.decode(encoded)
        && let Ok(payload) = serde_json::from_slice(&payload)
    {
        observation = observation.with_payload(payload);
    }
    observer
        .observe_domain_event(message.correlation_id().as_str(), observation)
        .await
}

async fn observe_integration_message(
    observer: &CorrelationObserver,
    message: &CorrelatedMessage,
) -> Result<(), CorrelationError> {
    let Ok(envelope) = serde_json::from_slice::<Value>(message.payload()) else {
        return Ok(());
    };
    let event_type = message
        .subject()
        .rsplit('.')
        .next()
        .unwrap_or("integration-event");
    let Some(schema_version) = envelope
        .get("schema_version")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
    else {
        return Ok(());
    };
    let mut observation = IntegrationEventObservation::new(event_type, schema_version)
        .with_subject(message.subject());
    if let Some(message_id) = message.message_id() {
        observation = observation.with_message_id(message_id.as_str());
    }
    if let Some(causation_id) = envelope.get("causation_id").and_then(Value::as_str) {
        observation = observation.with_causation_id(causation_id);
    }
    if let Some(payload) = envelope.get("payload") {
        observation = observation.with_payload(payload.clone());
    }
    observer
        .observe_integration_event(message.correlation_id().as_str(), observation)
        .await
}

#[async_trait]
impl TestScenarioReset for BikeRentalNatsRuntime {
    async fn reset(&self, fixture: &MaterializedTestFixture) -> Result<(), TestScenarioResetError> {
        self.reset_scope(fixture)
            .await
            .map_err(|error| TestScenarioResetError::Failed(error.to_string()))
    }
}
