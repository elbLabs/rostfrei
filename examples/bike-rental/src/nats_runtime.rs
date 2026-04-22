use std::time::Duration;

use async_trait::async_trait;
use rostfrei::{
    CommandDefinition, CommittedDomainEvent, CommittedEventContext, DomainEventHandler,
    DomainEventHandlerError, DomainEventHandlerErrorKind, EncodedIntegrationMessage,
    IntegrationEvent, IntegrationEventBus, IntegrationEventBusError, IntegrationEventBusErrorKind,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CommandAddress, ConsumerConfig, ContractError,
    DeliveryDisposition, IntegrationEventAddress, MessageDelivery, MessageHandler,
    QuarantineReason, RetryDelay,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, DomainEventConsumerError, NatsConnection,
    NatsDomainEventConsumerConfig, NatsError, NatsEventStore, NatsEventStoreConfig,
    provision_application_messaging, provision_domain_event_consumer, provision_durable_consumer,
    provision_event_store,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rental::{BicycleId, BicycleRented, FleetId, RentBicycle};

pub const DEFAULT_APPLICATION_NAME: &str = "bike-rental-demo";
pub const BOUNDED_CONTEXT_NAME: &str = "bike-rental";
pub const BICYCLE_RENTAL_STARTED_EVENT_NAME: &str = "bicycle-rental-started";
const BICYCLE_RENTAL_STARTED_SCHEMA_VERSION: u32 = 1;
const DOMAIN_EVENT_PUBLISHER_PURPOSE: &str = "bicycle-rental-started-publisher";
const INTEGRATION_EVENT_CONSUMER_PURPOSE: &str = "bicycle-rental-started-consumer";
const RETRY_DELAY: Duration = Duration::from_secs(1);
const CONSUMER_ACK_WAIT: Duration = Duration::from_secs(45);
const CONSUMER_PROCESSING_TIMEOUT: Duration = Duration::from_secs(30);
const CONSUMER_CONCURRENCY: usize = 4;
const MAXIMUM_DELIVERY_ATTEMPTS: u32 = 5;

#[derive(Debug, Error)]
pub enum BikeRentalNatsError {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    DomainEventConsumer(#[from] DomainEventConsumerError),
    #[error(transparent)]
    EventStore(#[from] rostfrei::EventStoreError),
    #[error(transparent)]
    Nats(#[from] NatsError),
}

#[derive(Clone)]
pub struct BikeRentalNatsConfig {
    application: ApplicationName,
    context: BoundedContext,
    messaging: ApplicationMessagingConfig,
    event_store: NatsEventStoreConfig,
    command_address: CommandAddress,
    command_consumer: ConsumerConfig<CommandAddress>,
    integration_event_address: IntegrationEventAddress,
    integration_event_consumer: ConsumerConfig<IntegrationEventAddress>,
    domain_event_consumer: NatsDomainEventConsumerConfig,
}

impl BikeRentalNatsConfig {
    pub fn new(application: &str) -> Result<Self, BikeRentalNatsError> {
        let application = ApplicationName::new(application)?;
        let context = application.bounded_context(BOUNDED_CONTEXT_NAME)?;
        let messaging = ApplicationMessagingConfig::new(&application)?;
        let event_store = NatsEventStoreConfig::for_bounded_context(&context)?;
        let command_address = context.command_address(RentBicycle::COMMAND_NAME)?;
        let command_consumer = ConsumerConfig::new(
            context.consumer_name(RentBicycle::COMMAND_NAME, RentBicycle::SCHEMA_VERSION)?,
            context.durable_name(RentBicycle::COMMAND_NAME, RentBicycle::SCHEMA_VERSION)?,
            command_address.clone(),
            CONSUMER_ACK_WAIT,
            CONSUMER_PROCESSING_TIMEOUT,
            CONSUMER_CONCURRENCY,
            MAXIMUM_DELIVERY_ATTEMPTS,
        )?;
        let integration_event_address =
            context.integration_event_address(BICYCLE_RENTAL_STARTED_EVENT_NAME)?;
        let integration_event_consumer = ConsumerConfig::new(
            context.consumer_name(INTEGRATION_EVENT_CONSUMER_PURPOSE, 1)?,
            context.durable_name(INTEGRATION_EVENT_CONSUMER_PURPOSE, 1)?,
            integration_event_address.clone(),
            CONSUMER_ACK_WAIT,
            CONSUMER_PROCESSING_TIMEOUT,
            CONSUMER_CONCURRENCY,
            MAXIMUM_DELIVERY_ATTEMPTS,
        )?;
        let domain_event_consumer = NatsDomainEventConsumerConfig::new(
            context.consumer_name(DOMAIN_EVENT_PUBLISHER_PURPOSE, 1)?,
            context.durable_name(DOMAIN_EVENT_PUBLISHER_PURPOSE, 1)?,
            CONSUMER_ACK_WAIT,
            CONSUMER_PROCESSING_TIMEOUT,
            RetryDelay::new(RETRY_DELAY)?,
        )?;
        Ok(Self {
            application,
            context,
            messaging,
            event_store,
            command_address,
            command_consumer,
            integration_event_address,
            integration_event_consumer,
            domain_event_consumer,
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
        &self.command_address
    }

    pub const fn command_consumer(&self) -> &ConsumerConfig<CommandAddress> {
        &self.command_consumer
    }

    pub const fn integration_event_address(&self) -> &IntegrationEventAddress {
        &self.integration_event_address
    }

    pub const fn integration_event_consumer(&self) -> &ConsumerConfig<IntegrationEventAddress> {
        &self.integration_event_consumer
    }

    pub const fn domain_event_consumer(&self) -> &NatsDomainEventConsumerConfig {
        &self.domain_event_consumer
    }

    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), BikeRentalNatsError> {
        provision_application_messaging(connection.jetstream(), &self.messaging).await?;
        provision_durable_consumer(
            connection.jetstream(),
            self.messaging.topology(),
            &self.command_consumer,
        )
        .await?;
        provision_durable_consumer(
            connection.jetstream(),
            self.messaging.topology(),
            &self.integration_event_consumer,
        )
        .await?;
        provision_event_store(connection.jetstream(), &self.event_store).await?;
        provision_domain_event_consumer(
            connection.jetstream(),
            &self.event_store,
            &self.domain_event_consumer,
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
