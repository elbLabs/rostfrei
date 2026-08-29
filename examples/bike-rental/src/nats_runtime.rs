use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei::{
    Aggregate, CommandDefinition, CommandExecutionError, CommandOutcome, ContentFingerprint,
    EventStore, EventStoreError, EventStoreErrorKind, ExecutionMetadata, Executor,
    JsonCommandPayload, OperationId as CoreOperationId,
};
use rostfrei_control_plane::{
    DispatchAdapter, DispatchError, DispatchErrorKind, DispatchInvocation, DispatchReceipt,
    dispatch_fingerprint,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CommandAddress, CommandEnvelope, CommandPublisher,
    ConsumerConfig, ContractError, DeliveryDisposition, EnvelopeContext, MAX_ENVELOPE_BYTES,
    MessageBuildError, MessageDelivery, MessageHandler, MessageId, MessageTimestamp, OperationId,
    OutboundMessage, PublishErrorKind, QuarantineReason, RetryDelay, SchemaVersion,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsError, NatsEventStore, NatsEventStoreConfig,
    provision_application_messaging, provision_durable_consumer, provision_event_store,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    rental::{RentBicycle, RentalFleetAggregate},
    runtime::rental_fleet_stream,
};

pub const DEFAULT_APPLICATION_NAME: &str = "bike-rental-demo";
pub const BOUNDED_CONTEXT_NAME: &str = "bike-rental";
const RETRY_DELAY: Duration = Duration::from_secs(1);
const CONSUMER_ACK_WAIT: Duration = Duration::from_secs(45);
const CONSUMER_PROCESSING_TIMEOUT: Duration = Duration::from_secs(30);
const CONSUMER_CONCURRENCY: usize = 4;
const MAXIMUM_DELIVERY_ATTEMPTS: u32 = 5;
const MAXIMUM_PUBLISH_ATTEMPTS: usize = 3;
const PUBLISH_RETRY_DELAY: Duration = Duration::from_millis(100);
const COMMAND_ENVELOPE_OVERHEAD: usize = 64 * 1024;
const MAXIMUM_COMMAND_PAYLOAD_LEN: usize =
    MAX_ENVELOPE_BYTES.saturating_sub(COMMAND_ENVELOPE_OVERHEAD);

#[derive(Debug, Error)]
pub enum BikeRentalNatsError {
    #[error(transparent)]
    Contract(#[from] ContractError),
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
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
        Ok(Self {
            application,
            context,
            messaging,
            event_store,
            command_address,
            command_consumer,
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

    pub async fn provision(&self, connection: &NatsConnection) -> Result<(), BikeRentalNatsError> {
        provision_application_messaging(connection.jetstream(), &self.messaging).await?;
        provision_durable_consumer(
            connection.jetstream(),
            self.messaging.topology(),
            &self.command_consumer,
        )
        .await?;
        provision_event_store(connection.jetstream(), &self.event_store).await?;
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
pub struct DispatchedCommand {
    aggregate_type: String,
    aggregate_id: String,
    command: String,
    schema_version: u32,
    payload: Value,
}

impl DispatchedCommand {
    fn from_invocation(invocation: &DispatchInvocation) -> Self {
        Self {
            aggregate_type: invocation.aggregate_type().to_owned(),
            aggregate_id: invocation.aggregate_id().as_str().to_owned(),
            command: invocation.command().to_owned(),
            schema_version: invocation.schema_version(),
            payload: invocation.payload().clone(),
        }
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub const fn payload(&self) -> &Value {
        &self.payload
    }
}

pub struct NatsCommandDispatchAdapter {
    publisher: Arc<dyn CommandPublisher>,
    address: CommandAddress,
}

impl NatsCommandDispatchAdapter {
    pub fn new(publisher: Arc<dyn CommandPublisher>, address: CommandAddress) -> Self {
        Self { publisher, address }
    }

    async fn publish(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<DispatchReceipt, DispatchError> {
        let mut attempts_remaining = MAXIMUM_PUBLISH_ATTEMPTS;
        loop {
            match self.publisher.publish_command(message.clone()).await {
                Ok(receipt) => return Ok(DispatchReceipt::new(receipt.duplicate())),
                Err(error)
                    if attempts_remaining > 1
                        && matches!(
                            error.kind(),
                            PublishErrorKind::Timeout | PublishErrorKind::Unavailable
                        ) =>
                {
                    attempts_remaining = attempts_remaining.saturating_sub(1);
                    tokio::time::sleep(PUBLISH_RETRY_DELAY).await;
                }
                Err(error) => return Err(publish_dispatch_error(error)),
            }
        }
    }
}

#[async_trait]
impl DispatchAdapter for NatsCommandDispatchAdapter {
    fn maximum_payload_len(&self) -> usize {
        MAXIMUM_COMMAND_PAYLOAD_LEN
    }

    async fn dispatch(
        &self,
        invocation: DispatchInvocation,
    ) -> Result<DispatchReceipt, DispatchError> {
        if invocation.aggregate_type() != RentalFleetAggregate::aggregate_type()
            || invocation.command() != RentBicycle::COMMAND_NAME
            || invocation.schema_version() != RentBicycle::SCHEMA_VERSION
            || invocation.operation_fingerprint()
                != dispatch_fingerprint(
                    invocation.aggregate_type(),
                    invocation.aggregate_id().as_str(),
                    invocation.command(),
                    invocation.schema_version(),
                    invocation.payload(),
                )
        {
            return Err(invalid_dispatch(
                "dispatch invocation does not match rent-bicycle",
            ));
        }

        let operation_id = messaging_operation_id(invocation.operation_id().as_str())?;
        let message_id =
            command_message_id(operation_id.as_str(), invocation.operation_fingerprint())
                .map_err(|error| contract_dispatch_error(&error))?;
        let envelope = CommandEnvelope::new(
            EnvelopeContext::new(
                message_id.clone(),
                messaging_schema_version(invocation.schema_version())?,
                rostfrei_messaging_core::CorrelationId::new(operation_id.as_str())
                    .map_err(|error| contract_dispatch_error(&error))?,
                None,
            ),
            operation_id,
            current_timestamp()?,
            DispatchedCommand::from_invocation(&invocation),
        )
        .map_err(|error| message_dispatch_error(&error))?;
        let message = OutboundMessage::json(self.address.clone(), message_id, &envelope)
            .map_err(|error| message_dispatch_error(&error))?;
        self.publish(message).await
    }
}

#[derive(Clone)]
pub struct RentBicycleMessageHandler<S> {
    store: S,
}

impl<S> RentBicycleMessageHandler<S> {
    pub const fn new(store: S) -> Self {
        Self { store }
    }
}

#[async_trait]
impl<S> MessageHandler<CommandAddress> for RentBicycleMessageHandler<S>
where
    S: EventStore + Clone + Send + Sync + 'static,
{
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        match execute_delivery(self.store.clone(), &delivery).await {
            Ok((operation_id, CommandOutcome::Accepted(receipt))) => {
                tracing::info!(
                    operation_id = %operation_id,
                    message_id = %delivery.message_id(),
                    events = receipt.events().len(),
                    exact_replay = receipt.is_exact_replay(),
                    "bike-rental command accepted"
                );
                DeliveryDisposition::Acknowledge
            }
            Ok((operation_id, CommandOutcome::Rejected(_))) => {
                tracing::info!(
                    operation_id = %operation_id,
                    message_id = %delivery.message_id(),
                    "bike-rental command rejected"
                );
                DeliveryDisposition::Acknowledge
            }
            Err(DeliveryFailure::Transient) => retry_disposition(),
            Err(DeliveryFailure::Invalid) => {
                quarantine_disposition("invalid bike-rental command envelope")
            }
        }
    }
}

#[derive(Clone, Copy)]
enum DeliveryFailure {
    Invalid,
    Transient,
}

async fn execute_delivery<S>(
    store: S,
    delivery: &MessageDelivery<CommandAddress>,
) -> Result<
    (
        OperationId,
        CommandOutcome<<RentBicycle as rostfrei::DomainCommandType>::Rejection>,
    ),
    DeliveryFailure,
>
where
    S: EventStore + Clone,
{
    let envelope: CommandEnvelope<DispatchedCommand> =
        serde_json::from_slice(delivery.payload()).map_err(|_| DeliveryFailure::Invalid)?;
    if envelope.message_id() != delivery.message_id()
        || envelope.schema_version().get() != RentBicycle::SCHEMA_VERSION
        || envelope.payload().aggregate_type != RentalFleetAggregate::aggregate_type()
        || envelope.payload().command != RentBicycle::COMMAND_NAME
        || envelope.payload().schema_version != RentBicycle::SCHEMA_VERSION
        || delivery.address().name() != RentBicycle::COMMAND_NAME
    {
        return Err(DeliveryFailure::Invalid);
    }
    let dispatched = envelope.payload();
    let fingerprint = dispatch_fingerprint(
        &dispatched.aggregate_type,
        &dispatched.aggregate_id,
        &dispatched.command,
        dispatched.schema_version,
        &dispatched.payload,
    );
    let expected_message_id = command_message_id(envelope.operation_id().as_str(), fingerprint)
        .map_err(|_| DeliveryFailure::Invalid)?;
    if delivery.message_id() != &expected_message_id {
        return Err(DeliveryFailure::Invalid);
    }
    let stream =
        rental_fleet_stream(&dispatched.aggregate_id).map_err(|_| DeliveryFailure::Invalid)?;
    let command =
        RentBicycle::decode_json(&dispatched.payload).map_err(|_| DeliveryFailure::Invalid)?;
    let operation_id = envelope.operation_id().clone();
    let core_operation_id =
        CoreOperationId::new(operation_id.as_str()).map_err(|_| DeliveryFailure::Invalid)?;
    let mut metadata = ExecutionMetadata::new(stream, core_operation_id, fingerprint)
        .with_correlation_id(envelope.correlation_id().clone());
    if let Some(causation_id) = envelope.causation_id() {
        metadata = metadata.with_causation_id(causation_id.clone());
    }
    let outcome = Executor::new(store)
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await
        .map_err(classify_execution_error)?;
    Ok((operation_id, outcome))
}

fn classify_execution_error(error: CommandExecutionError) -> DeliveryFailure {
    match error {
        CommandExecutionError::Store(error)
            if matches!(
                error.kind(),
                EventStoreErrorKind::InvalidRequest | EventStoreErrorKind::IdentityConflict
            ) =>
        {
            DeliveryFailure::Invalid
        }
        CommandExecutionError::Store(_) | CommandExecutionError::Codec(_) => {
            DeliveryFailure::Transient
        }
    }
}

fn retry_disposition() -> DeliveryDisposition {
    RetryDelay::new(RETRY_DELAY).map_or(
        DeliveryDisposition::Terminate,
        DeliveryDisposition::RetryAfter,
    )
}

fn quarantine_disposition(reason: &'static str) -> DeliveryDisposition {
    QuarantineReason::new(reason).map_or(
        DeliveryDisposition::Terminate,
        DeliveryDisposition::Quarantine,
    )
}

fn messaging_operation_id(value: &str) -> Result<OperationId, DispatchError> {
    OperationId::new(value).map_err(|error| contract_dispatch_error(&error))
}

fn messaging_schema_version(value: u32) -> Result<SchemaVersion, DispatchError> {
    SchemaVersion::new(value).map_err(|error| contract_dispatch_error(&error))
}

fn command_message_id(
    operation_id: &str,
    operation_fingerprint: ContentFingerprint,
) -> Result<MessageId, ContractError> {
    let identity = format!(
        "rostfrei:dispatch-message:v1:{operation_id}:{}",
        operation_fingerprint.to_hex()
    );
    MessageId::new(ContentFingerprint::digest(identity).to_hex())
}

fn current_timestamp() -> Result<MessageTimestamp, DispatchError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid_dispatch("system clock is before the Unix epoch"))?
        .as_millis();
    let milliseconds = u64::try_from(milliseconds)
        .map_err(|_| invalid_dispatch("system clock is outside the command timestamp range"))?;
    MessageTimestamp::from_unix_milliseconds(milliseconds)
        .map_err(|error| contract_dispatch_error(&error))
}

fn contract_dispatch_error(error: &ContractError) -> DispatchError {
    invalid_dispatch(error.to_string())
}

fn message_dispatch_error(error: &MessageBuildError) -> DispatchError {
    invalid_dispatch(error.to_string())
}

fn invalid_dispatch(message: impl Into<String>) -> DispatchError {
    DispatchError::new(DispatchErrorKind::InvalidRequest, message)
}

fn publish_dispatch_error(error: rostfrei_messaging_core::PublishError) -> DispatchError {
    let kind = match error.kind() {
        PublishErrorKind::Rejected => DispatchErrorKind::Rejected,
        PublishErrorKind::Timeout => DispatchErrorKind::Timeout,
        PublishErrorKind::InvalidConfiguration => DispatchErrorKind::InvalidConfiguration,
        _ => DispatchErrorKind::Unavailable,
    };
    DispatchError::new(kind, error.to_string())
}

pub fn execution_fingerprint(command: &DispatchedCommand) -> ContentFingerprint {
    dispatch_fingerprint(
        &command.aggregate_type,
        &command.aggregate_id,
        &command.command,
        command.schema_version,
        &command.payload,
    )
}
