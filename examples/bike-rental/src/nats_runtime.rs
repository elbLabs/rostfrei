use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei::{
    Aggregate, CommandDefinition, CommandExecutionError, CommandOutcome, ContentFingerprint,
    DomainErrorType, EventStore, EventStoreError, EventStoreErrorKind, ExecutionMetadata, Executor,
    JsonCommandPayload, JsonErrorPayload, OperationId as CoreOperationId, StreamId,
};
use rostfrei_control_plane::{
    DispatchAdapter, DispatchError, DispatchErrorKind, DispatchInvocation, DispatchObserver,
    DispatchPublication, DispatchReceipt, DispatchRejection, dispatch_fingerprint,
};
use rostfrei_messaging_core::{
    ApplicationErrorCode, ApplicationName, BoundedContext, COMMAND_RESPONSE_SCHEMA_VERSION,
    CausationId, CommandAddress, CommandEnvelope, CommandPublisher, CommandRejection,
    CommandRejectionClassification, CommandResponse, CommandResponseAddress,
    CommandResponseOutcome, CommandResponsePublisher, CommandResponseReadError,
    CommandResponseReadErrorKind, CommandResponseReader, ConsumerConfig, ContractError,
    DeliveryDisposition, EnvelopeContext, MAX_ENVELOPE_BYTES, MessageBuildError, MessageDelivery,
    MessageHandler, MessageId, MessageTimestamp, OperationId, OutboundMessage, PublishError,
    PublishErrorKind, PublishReceipt, QuarantineReason, RetryDelay, SchemaVersion,
    derive_command_response_address,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsError, NatsEventStore, NatsEventStoreConfig,
    provision_application_messaging, provision_durable_consumer, provision_event_store,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    rental::{BicycleUnavailable, RentBicycle, RentalFleetAggregate},
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
const COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_RESPONSE_RECONCILIATION_TIMEOUT: Duration = Duration::from_millis(100);
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
    response_reader: Arc<dyn CommandResponseReader>,
    address: CommandAddress,
}

impl NatsCommandDispatchAdapter {
    pub fn new(
        publisher: Arc<dyn CommandPublisher>,
        response_reader: Arc<dyn CommandResponseReader>,
        address: CommandAddress,
    ) -> Self {
        Self {
            publisher,
            response_reader,
            address,
        }
    }

    async fn publish(
        &self,
        message: OutboundMessage<CommandAddress>,
    ) -> Result<PublishReceipt, DispatchError> {
        let mut attempts_remaining = MAXIMUM_PUBLISH_ATTEMPTS;
        loop {
            match self.publisher.publish_command(message.clone()).await {
                Ok(receipt) => return Ok(receipt),
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

    async fn read_response(
        &self,
        response_address: &CommandResponseAddress,
        operation_id: &OperationId,
        command_message_id: &MessageId,
    ) -> Result<CommandResponse, DispatchError> {
        loop {
            match self
                .response_reader
                .read_command_response(
                    response_address,
                    operation_id,
                    command_message_id,
                    COMMAND_RESPONSE_TIMEOUT,
                )
                .await
            {
                Ok(response) => return Ok(response),
                Err(error)
                    if matches!(
                        error.kind(),
                        CommandResponseReadErrorKind::Timeout
                            | CommandResponseReadErrorKind::Unavailable
                    ) =>
                {
                    if error.kind() == CommandResponseReadErrorKind::Unavailable {
                        tokio::time::sleep(PUBLISH_RETRY_DELAY).await;
                    }
                }
                Err(error) => return Err(response_read_dispatch_error(error)),
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
        observer: Arc<dyn DispatchObserver>,
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
        let correlation_id = rostfrei_messaging_core::CorrelationId::new(operation_id.as_str())
            .map_err(|error| contract_dispatch_error(&error))?;
        let envelope = CommandEnvelope::new(
            EnvelopeContext::new(
                message_id.clone(),
                messaging_schema_version(invocation.schema_version())?,
                correlation_id.clone(),
                None,
            ),
            operation_id,
            current_timestamp()?,
            DispatchedCommand::from_invocation(&invocation),
        )
        .map_err(|error| message_dispatch_error(&error))?;
        let message = OutboundMessage::json(self.address.clone(), message_id, &envelope)
            .map_err(|error| message_dispatch_error(&error))?;
        let publication = self.publish(message).await?;
        observer
            .command_published(DispatchPublication::new(
                envelope.message_id().as_str(),
                publication.duplicate(),
            ))
            .await;
        let response_address = derive_command_response_address(
            &self.address,
            envelope.operation_id(),
            envelope.message_id(),
        )
        .map_err(|error| contract_dispatch_error(&error))?;
        let response = self
            .read_response(
                &response_address,
                envelope.operation_id(),
                envelope.message_id(),
            )
            .await?;
        if response.command_address() != &self.address
            || response.operation_id() != envelope.operation_id()
            || response.command_message_id() != envelope.message_id()
            || response.schema_version().get() != COMMAND_RESPONSE_SCHEMA_VERSION
            || response.correlation_id() != &correlation_id
        {
            return Err(DispatchError::new(
                DispatchErrorKind::InvalidResponse,
                "command response context does not match the command",
            ));
        }
        dispatch_receipt(response, publication.duplicate())
    }
}

#[derive(Clone)]
pub struct RentBicycleMessageHandler<S> {
    store: S,
    response_publisher: Arc<dyn CommandResponsePublisher>,
    response_reader: Arc<dyn CommandResponseReader>,
}

impl<S> RentBicycleMessageHandler<S> {
    pub fn new(
        store: S,
        response_publisher: Arc<dyn CommandResponsePublisher>,
        response_reader: Arc<dyn CommandResponseReader>,
    ) -> Self {
        Self {
            store,
            response_publisher,
            response_reader,
        }
    }

    async fn publish_response(
        &self,
        message: OutboundMessage<rostfrei_messaging_core::CommandResponseAddress>,
    ) -> Result<PublishReceipt, PublishError>
    where
        S: Sync,
    {
        let mut attempts_remaining = MAXIMUM_PUBLISH_ATTEMPTS;
        loop {
            match self
                .response_publisher
                .publish_command_response(message.clone())
                .await
            {
                Ok(receipt) => return Ok(receipt),
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
                Err(error) => return Err(error),
            }
        }
    }
}

#[async_trait]
impl<S> MessageHandler<CommandAddress> for RentBicycleMessageHandler<S>
where
    S: EventStore + Clone + Send + Sync + 'static,
{
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        let prepared = match prepare_delivery(&delivery) {
            Ok(prepared) => prepared,
            Err(DeliveryFailure::Invalid) => {
                return quarantine_disposition("invalid bike-rental command envelope");
            }
            Err(DeliveryFailure::Transient | DeliveryFailure::MandatoryResponse) => {
                return retry_disposition();
            }
        };
        match reconcile_persisted_response(self.response_reader.as_ref(), &delivery, &prepared)
            .await
        {
            Ok(Some(response)) => {
                tracing::info!(
                    operation_id = %prepared.envelope.operation_id(),
                    message_id = %delivery.message_id(),
                    response_message_id = %response.message_id(),
                    "bike-rental command response already persisted"
                );
                return DeliveryDisposition::Acknowledge;
            }
            Ok(None) => {}
            Err(DeliveryFailure::Transient | DeliveryFailure::MandatoryResponse) => {
                return retry_disposition();
            }
            Err(DeliveryFailure::Invalid) => {
                return quarantine_disposition("invalid bike-rental command response");
            }
        }

        match execute_delivery(self.store.clone(), &delivery, prepared).await {
            Ok(executed) => {
                let operation_id = executed.operation_id.clone();
                let response_message_id = executed.response.message_id().clone();
                let Ok(response) = OutboundMessage::json(
                    executed.response_address,
                    response_message_id.clone(),
                    &executed.response,
                ) else {
                    return retry_disposition();
                };
                match self.publish_response(response).await {
                    Ok(publication) => {
                        match executed.summary {
                            ExecutionSummary::Accepted {
                                events,
                                exact_replay,
                            } => tracing::info!(
                                operation_id = %operation_id,
                                message_id = %delivery.message_id(),
                                response_message_id = %response_message_id,
                                response_duplicate = publication.duplicate(),
                                events,
                                exact_replay,
                                "bike-rental command accepted"
                            ),
                            ExecutionSummary::Rejected => tracing::info!(
                                operation_id = %operation_id,
                                message_id = %delivery.message_id(),
                                response_message_id = %response_message_id,
                                response_duplicate = publication.duplicate(),
                                "bike-rental command rejected"
                            ),
                        }
                        DeliveryDisposition::Acknowledge
                    }
                    Err(error) => {
                        tracing::warn!(
                            operation_id = %operation_id,
                            message_id = %delivery.message_id(),
                            response_message_id = %response_message_id,
                            error = %error,
                            "bike-rental command response publication failed"
                        );
                        retry_disposition()
                    }
                }
            }
            Err(DeliveryFailure::Transient | DeliveryFailure::MandatoryResponse) => {
                retry_disposition()
            }
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
    MandatoryResponse,
}

struct ExecutedDelivery {
    operation_id: OperationId,
    response_address: CommandResponseAddress,
    response: CommandResponse,
    summary: ExecutionSummary,
}

enum ExecutionSummary {
    Accepted { events: usize, exact_replay: bool },
    Rejected,
}

struct PreparedDelivery {
    envelope: CommandEnvelope<DispatchedCommand>,
    command: RentBicycle,
    stream: StreamId,
    fingerprint: ContentFingerprint,
    response_address: CommandResponseAddress,
    response_message_id: MessageId,
}

fn prepare_delivery(
    delivery: &MessageDelivery<CommandAddress>,
) -> Result<PreparedDelivery, DeliveryFailure> {
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
    let response_address = derive_command_response_address(
        delivery.address(),
        envelope.operation_id(),
        delivery.message_id(),
    )
    .map_err(|_| DeliveryFailure::MandatoryResponse)?;
    let response_message_id = command_response_message_id(delivery.message_id())
        .map_err(|_| DeliveryFailure::MandatoryResponse)?;
    Ok(PreparedDelivery {
        envelope,
        command,
        stream,
        fingerprint,
        response_address,
        response_message_id,
    })
}

async fn reconcile_persisted_response(
    response_reader: &dyn CommandResponseReader,
    delivery: &MessageDelivery<CommandAddress>,
    prepared: &PreparedDelivery,
) -> Result<Option<CommandResponse>, DeliveryFailure> {
    match response_reader
        .read_command_response(
            &prepared.response_address,
            prepared.envelope.operation_id(),
            delivery.message_id(),
            COMMAND_RESPONSE_RECONCILIATION_TIMEOUT,
        )
        .await
    {
        Ok(response)
            if response.message_id() == &prepared.response_message_id
                && response.command_message_id() == delivery.message_id()
                && response.command_address() == delivery.address()
                && response.operation_id() == prepared.envelope.operation_id()
                && response.schema_version().get() == COMMAND_RESPONSE_SCHEMA_VERSION
                && response.correlation_id() == prepared.envelope.correlation_id() =>
        {
            Ok(Some(response))
        }
        Ok(_) => Err(DeliveryFailure::Invalid),
        Err(error) => match error.kind() {
            CommandResponseReadErrorKind::Timeout => Ok(None),
            CommandResponseReadErrorKind::Unavailable => Err(DeliveryFailure::Transient),
            _ => Err(DeliveryFailure::Invalid),
        },
    }
}

async fn execute_delivery<S>(
    store: S,
    delivery: &MessageDelivery<CommandAddress>,
    prepared: PreparedDelivery,
) -> Result<ExecutedDelivery, DeliveryFailure>
where
    S: EventStore + Clone,
{
    let PreparedDelivery {
        envelope,
        command,
        stream,
        fingerprint,
        response_address,
        response_message_id,
    } = prepared;
    let operation_id = envelope.operation_id().clone();
    let core_operation_id =
        CoreOperationId::new(operation_id.as_str()).map_err(|_| DeliveryFailure::Invalid)?;
    let command_causation_id =
        CausationId::new(delivery.message_id().as_str()).map_err(|_| DeliveryFailure::Invalid)?;
    let metadata = ExecutionMetadata::new(stream, core_operation_id, fingerprint)
        .with_correlation_id(envelope.correlation_id().clone())
        .with_causation_id(command_causation_id);
    let outcome = Executor::new(store)
        .execute::<RentalFleetAggregate, _>(metadata, &command)
        .await
        .map_err(classify_execution_error)?;
    let correlation_id = envelope.correlation_id().clone();
    let (response, summary) = match outcome {
        CommandOutcome::Accepted(receipt) => (
            CommandResponse::accepted(
                response_message_id,
                delivery.message_id().clone(),
                delivery.address().clone(),
                operation_id.clone(),
                correlation_id,
            )
            .map_err(|_| DeliveryFailure::MandatoryResponse)?,
            ExecutionSummary::Accepted {
                events: receipt.events().len(),
                exact_replay: receipt.is_exact_replay(),
            },
        ),
        CommandOutcome::Rejected(rejection) => {
            let descriptor = <BicycleUnavailable as DomainErrorType>::DESCRIPTOR;
            let details = rejection
                .encode_json()
                .map_err(|_| DeliveryFailure::MandatoryResponse)?;
            let rejection = CommandRejection::new(
                CommandRejectionClassification::Conflict,
                ApplicationErrorCode::new(descriptor.code)
                    .map_err(|_| DeliveryFailure::MandatoryResponse)?,
                descriptor.message,
                Some(details),
            )
            .map_err(|_| DeliveryFailure::MandatoryResponse)?;
            (
                CommandResponse::rejected(
                    response_message_id,
                    delivery.message_id().clone(),
                    delivery.address().clone(),
                    operation_id.clone(),
                    correlation_id,
                    rejection,
                )
                .map_err(|_| DeliveryFailure::MandatoryResponse)?,
                ExecutionSummary::Rejected,
            )
        }
    };
    Ok(ExecutedDelivery {
        operation_id,
        response_address,
        response,
        summary,
    })
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

fn command_response_message_id(command_message_id: &MessageId) -> Result<MessageId, ContractError> {
    let identity = format!(
        "rostfrei:command-response-message:v1:{}",
        command_message_id.as_str()
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

fn response_read_dispatch_error(error: CommandResponseReadError) -> DispatchError {
    let kind = match error.kind() {
        CommandResponseReadErrorKind::Timeout => DispatchErrorKind::Timeout,
        CommandResponseReadErrorKind::Unavailable => DispatchErrorKind::Unavailable,
        CommandResponseReadErrorKind::InvalidConfiguration => {
            DispatchErrorKind::InvalidConfiguration
        }
        _ => DispatchErrorKind::InvalidResponse,
    };
    DispatchError::new(kind, error.to_string())
}

fn dispatch_receipt(
    response: CommandResponse,
    duplicate: bool,
) -> Result<DispatchReceipt, DispatchError> {
    let command_message_id = response.command_message_id().as_str().to_owned();
    let response_message_id = response.message_id().as_str().to_owned();
    match response.into_outcome() {
        CommandResponseOutcome::Accepted => Ok(DispatchReceipt::accepted(
            command_message_id,
            response_message_id,
            duplicate,
        )),
        CommandResponseOutcome::Rejected(rejection) => {
            let rejection = serde_json::from_value::<DispatchRejection>(
                serde_json::to_value(rejection).map_err(|_| {
                    DispatchError::new(
                        DispatchErrorKind::InvalidResponse,
                        "command rejection could not be represented",
                    )
                })?,
            )
            .map_err(|_| {
                DispatchError::new(
                    DispatchErrorKind::InvalidResponse,
                    "command rejection could not be represented",
                )
            })?;
            Ok(DispatchReceipt::rejected(
                command_message_id,
                response_message_id,
                duplicate,
                rejection,
            ))
        }
    }
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
