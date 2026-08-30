use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use rostfrei::{
    Aggregate, CommandBindingRegistrationError, CommandBus, CommandDefinition,
    CommandMessageAdapter, CommandProcessor, CommittedDomainEvent, CommittedEventContext,
    DomainEventDefinitionType, DomainEventDispatcher, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, DomainEventRegistrationError, EncodedIntegrationMessage,
    EventCodec, EventId, EventStore, InfallibleCommandRejectionMapper, IntegrationEvent,
    IntegrationEventBus, IntegrationEventBusError, IntegrationEventBusErrorKind,
    IntegrationMessageAdapter, JsonDomainRejectionMapper, JsonEventCodec, integration_message_id,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CommandAddress, CommandRejectionClassification, ConsumeError,
    ConsumerConfig, ContractError, DeliveryDisposition, IntegrationEventAddress, MessageDelivery,
    MessageHandler, MessageId, QuarantineReason, RetryDelay, SchemaVersion,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, CorrelatedMessage, CorrelatedMessageFamily,
    CorrelatedMessageHandler, DomainEventConsumerError, NatsConnection, NatsCorrelationObserver,
    NatsDomainEventConsumer, NatsDomainEventConsumerConfig, NatsError, NatsEventStore,
    NatsEventStoreConfig, NatsMessagingAdapter, decode_consumed_event,
    provision_application_messaging, provision_domain_event_consumer, provision_durable_consumer,
    provision_event_store,
};
use rostfrei_tracer::{
    CommandBusTransportAdapter, CommandInvocation, CommandReceipt, CommandTransport,
    CommandTransportError, CommandTransportObserver, CorrelationError, CorrelationObserver,
    DomainEventObservation, IntegrationEventObservation, TestScenarioReset, TestScenarioResetError,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    sync::{Mutex, RwLock, watch},
    task::JoinHandle,
};

use crate::{
    rental::{
        AddBicycle, BicycleId, BicycleRented, FleetId, RentBicycle, RentalFleetAggregate,
        ReturnBicycle,
    },
    runtime::{SeedError, seed_demo},
};

pub const DEFAULT_APPLICATION_NAME: &str = "bike-rental-demo";
pub const BOUNDED_CONTEXT_NAME: &str = "bike-rental";
pub const TEST_APPLICATION_NAME: &str = "bike-rental-test";
pub const PRODUCTION_APPLICATION_NAME: &str = "bike-rental-prod";
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
const MESSAGING_STREAM_MAX_BYTES: i64 = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum BikeRentalNatsError {
    #[error(transparent)]
    CommandBindingRegistration(#[from] CommandBindingRegistrationError),
    #[error(transparent)]
    Consume(#[from] ConsumeError),
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    DomainEventConsumer(#[from] DomainEventConsumerError),
    #[error(transparent)]
    DomainEventRegistration(#[from] DomainEventRegistrationError),
    #[error(transparent)]
    EventStore(#[from] rostfrei::EventStoreError),
    #[error(transparent)]
    Nats(#[from] NatsError),
    #[error(transparent)]
    Seed(#[from] SeedError),
    #[error("bike-rental NATS runtime failed: {0}")]
    Runtime(String),
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
    messaging: ApplicationMessagingConfig,
    event_store: NatsEventStoreConfig,
    domain_event_consumer: NatsDomainEventConsumerConfig,
    command_routes: [BikeRentalCommandRoute; 3],
    integration_event_route: BikeRentalIntegrationEventRoute,
}

impl BikeRentalNatsConfig {
    pub fn new(application: &str) -> Result<Self, BikeRentalNatsError> {
        let application = ApplicationName::new(application)?;
        let context = application.bounded_context(BOUNDED_CONTEXT_NAME)?;
        let messaging = ApplicationMessagingConfig::new(&application)?
            .with_max_bytes(MESSAGING_STREAM_MAX_BYTES)?;
        let event_store = NatsEventStoreConfig::for_bounded_context(&context)?;
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

    pub const fn messaging(&self) -> &ApplicationMessagingConfig {
        &self.messaging
    }

    pub const fn event_store(&self) -> &NatsEventStoreConfig {
        &self.event_store
    }

    pub const fn command_address(&self) -> &CommandAddress {
        self.command_route(BikeRentalCommand::RentBicycle).address()
    }

    pub const fn command_consumer(&self) -> &ConsumerConfig<CommandAddress> {
        self.command_route(BikeRentalCommand::RentBicycle)
            .consumer()
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

    pub const fn integration_event_address(&self) -> &IntegrationEventAddress {
        self.integration_event_route.address()
    }

    pub const fn integration_event_consumer(&self) -> &ConsumerConfig<IntegrationEventAddress> {
        self.integration_event_route.consumer()
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
        let occurred_at = event.recorded().committed_at().ok_or_else(|| {
            DomainEventHandlerError::new(
                DomainEventHandlerErrorKind::InvalidCommittedEvent,
                "BicycleRented has no stable commit timestamp",
            )
        })?;
        let committed = CommittedEventContext::new(event.recorded())
            .map_err(|error| classify_integration_event_error(&error))?
            .with_occurred_at(occurred_at);
        let integration_event = BicycleRentalStarted {
            source_event_id: event.recorded().event_id().as_str().to_owned(),
            fleet_id: event.event().fleet_id.clone(),
            bicycle_id: event.event().bicycle_id.clone(),
        };
        self.bus
            .publish(committed, integration_event)
            .await
            .map_err(|error| classify_integration_event_error(&error))?;
        Ok(())
    }
}

pub fn bicycle_rental_started_message_id(
    address: &IntegrationEventAddress,
    event_id: &EventId,
) -> Result<MessageId, IntegrationEventBusError> {
    let schema_version =
        SchemaVersion::new(BICYCLE_RENTAL_STARTED_SCHEMA_VERSION).map_err(|error| {
            IntegrationEventBusError::new(
                IntegrationEventBusErrorKind::InvalidMessage,
                error.to_string(),
            )
        })?;
    integration_message_id(address, schema_version, event_id)
}

#[derive(Clone)]
pub struct BicycleRentalStartedHandler {
    address: IntegrationEventAddress,
}

impl BicycleRentalStartedHandler {
    pub const fn new(address: IntegrationEventAddress) -> Self {
        Self { address }
    }
}

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
        )
        .and_then(|message| message.decode::<BicycleRentalStarted>());
        let Ok(envelope) = envelope else {
            return quarantine("invalid bicycle-rental-started envelope");
        };
        if delivery.address() != &self.address
            || delivery.correlation_id() != Some(envelope.correlation_id())
        {
            return quarantine("invalid bicycle-rental-started envelope");
        }
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

fn quarantine(reason: &'static str) -> DeliveryDisposition {
    QuarantineReason::new(reason).map_or(
        DeliveryDisposition::Terminate,
        DeliveryDisposition::Quarantine,
    )
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
    fn new(inner: Arc<dyn CommandTransport>, scope_gate: Arc<RwLock<()>>) -> Self {
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
    ) -> Result<CommandReceipt, CommandTransportError> {
        let _scope = Arc::clone(&self.scope_gate).read_owned().await;
        self.inner.invoke(invocation, observer).await
    }
}

pub struct BikeRentalNatsRuntime {
    connection: NatsConnection,
    config: BikeRentalNatsConfig,
    store: NatsEventStore,
    messaging: Arc<NatsMessagingAdapter>,
    transport: Arc<dyn CommandTransport>,
    scope_gate: Arc<RwLock<()>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

struct TracerCorrelationHandler {
    observer: CorrelationObserver,
    event_store: NatsEventStoreConfig,
    integration_event_address: IntegrationEventAddress,
}

#[async_trait]
impl CorrelatedMessageHandler for TracerCorrelationHandler {
    async fn handle(&self, message: CorrelatedMessage) {
        let result = match message.family() {
            CorrelatedMessageFamily::DomainEvent => {
                observe_domain_message(&self.observer, &self.event_store, &message).await
            }
            CorrelatedMessageFamily::IntegrationEvent => {
                observe_integration_message(
                    &self.observer,
                    &self.integration_event_address,
                    &message,
                )
                .await
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
        let config = BikeRentalNatsConfig::new(application)?;
        config.provision(&connection).await?;
        let store = config.connect_store(&connection).await?;
        let messaging = Arc::new(connection.messaging_adapter(config.messaging.topology().clone()));
        let command_adapter: Arc<dyn CommandMessageAdapter> = messaging.clone();
        let command_bus = CommandBus::new(config.context().clone(), command_adapter);
        let command_transport: Arc<dyn CommandTransport> =
            Arc::new(CommandBusTransportAdapter::new(command_bus));
        let scope_gate = Arc::new(RwLock::new(()));
        let transport: Arc<dyn CommandTransport> = Arc::new(ScopedCommandTransport::new(
            command_transport,
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
        Arc::clone(&self.transport)
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

        let event_store: Arc<dyn EventStore> = Arc::new(self.store.clone());
        let mut processor = CommandProcessor::new(event_store);
        processor.register::<RentBicycle, _>(JsonDomainRejectionMapper::new(
            CommandRejectionClassification::Conflict,
        ))?;
        processor.register::<ReturnBicycle, _>(JsonDomainRejectionMapper::new(
            CommandRejectionClassification::Conflict,
        ))?;
        processor.register::<AddBicycle, _>(InfallibleCommandRejectionMapper)?;
        let command_handler: Arc<dyn MessageHandler<CommandAddress>> =
            Arc::new(self.messaging.command_handler(Arc::new(processor)));

        let factory = self
            .connection
            .consumer_factory(self.config.messaging.topology().clone());
        let mut prepared = Vec::with_capacity(self.config.command_routes.len());
        for route in &self.config.command_routes {
            factory.verify_consumer(&route.consumer).await?;
            let consumer = rostfrei_messaging_core::MessageConsumerFactory::create(
                &factory,
                route.consumer.clone(),
            )?;
            prepared.push((route.command, consumer, Arc::clone(&command_handler)));
        }

        let integration_route = self.config.integration_event_route();
        factory.verify_consumer(&integration_route.consumer).await?;
        let integration_consumer = rostfrei_messaging_core::MessageConsumerFactory::create(
            &factory,
            integration_route.consumer.clone(),
        )?;
        let integration_handler: Arc<dyn MessageHandler<IntegrationEventAddress>> = Arc::new(
            BicycleRentalStartedHandler::new(integration_route.address.clone()),
        );

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
        for (command, consumer, handler) in prepared {
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
        let domain_event_stream = self.config.event_store().stream_name();
        let integration_event_stream = self
            .config
            .messaging()
            .topology()
            .integration_event_stream()
            .as_str();
        let nats_observer = NatsCorrelationObserver::new(
            self.connection.client().clone(),
            self.config.application().clone(),
        )
        .with_streams(domain_event_stream, integration_event_stream);
        let subscription = nats_observer.subscribe().await?;
        let handler = Arc::new(TracerCorrelationHandler {
            observer,
            event_store: self.config.event_store().clone(),
            integration_event_address: self.config.integration_event_address().clone(),
        });
        Ok(tokio::spawn(async move {
            if let Err(error) = subscription.run(handler).await {
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
            self.connection
                .delete_stream_if_exists(stream)
                .await
                .map_err(|error| BikeRentalNatsError::Runtime(error.to_string()))?;
        }
        Ok(())
    }

    async fn reset_scope(&self) -> Result<(), BikeRentalNatsError> {
        let _scope = Arc::clone(&self.scope_gate).write_owned().await;
        self.stop_workers_in_scope().await;
        self.delete_resources().await?;
        self.config.provision(&self.connection).await?;
        self.seed_demo().await?;
        self.start_workers_in_scope().await
    }
}

async fn observe_domain_message(
    observer: &CorrelationObserver,
    event_store: &NatsEventStoreConfig,
    message: &CorrelatedMessage,
) -> Result<(), CorrelationError> {
    let Ok(decoded) = decode_consumed_event(
        event_store,
        message.subject(),
        message.headers(),
        message.payload(),
    ) else {
        return Ok(());
    };
    let recorded = decoded.recorded;
    if recorded.stream_id().aggregate_type().as_str()
        != RentalFleetAggregate::aggregate_type().as_ref()
        || <JsonEventCodec as EventCodec<RentalFleetAggregate>>::decode(&JsonEventCodec, &recorded)
            .is_err()
        || recorded
            .correlation_id()
            .map(rostfrei::CorrelationId::as_str)
            != Some(message.correlation_id().as_str())
    {
        return Ok(());
    }
    let mut observation =
        DomainEventObservation::new(recorded.event_type(), recorded.schema_version())
            .with_stream_version(recorded.stream_version().value());
    if let Ok(payload) = serde_json::from_slice(recorded.payload()) {
        observation = observation.with_payload(payload);
    }
    observer
        .observe_domain_event(message.correlation_id().as_str(), observation)
        .await
}

async fn observe_integration_message(
    observer: &CorrelationObserver,
    address: &IntegrationEventAddress,
    message: &CorrelatedMessage,
) -> Result<(), CorrelationError> {
    if message.subject() != address.as_str() {
        return Ok(());
    }
    let Some(message_id) = message.message_id() else {
        return Ok(());
    };
    let Ok(envelope) = EncodedIntegrationMessage::from_delivery(
        address.clone(),
        message_id.clone(),
        message.payload().to_vec(),
    )
    .and_then(|message| message.decode::<BicycleRentalStarted>()) else {
        return Ok(());
    };
    if envelope.correlation_id() != message.correlation_id() {
        return Ok(());
    }
    let mut observation = IntegrationEventObservation::new(
        BICYCLE_RENTAL_STARTED_EVENT_NAME,
        envelope.schema_version().get(),
    )
    .with_subject(message.subject())
    .with_message_id(message_id.as_str());
    if let Ok(payload) = serde_json::to_value(envelope.payload()) {
        observation = observation.with_payload(payload);
    }
    observer
        .observe_integration_event(message.correlation_id().as_str(), observation)
        .await
}

#[async_trait]
impl TestScenarioReset for BikeRentalNatsRuntime {
    async fn reset(&self) -> Result<(), TestScenarioResetError> {
        self.reset_scope()
            .await
            .map_err(|error| TestScenarioResetError::Failed(error.to_string()))
    }
}
