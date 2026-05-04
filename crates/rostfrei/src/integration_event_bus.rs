use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use domain::DomainEvent;
use rostfrei_core::{
    Aggregate, CommittedDomainEvent, DomainEventDispatcher, DomainEventHandler,
    DomainEventHandlerError, DomainEventHandlerErrorKind, DomainEventRegistrationError, Event,
    EventId, EventVariant, RecordedEvent,
};
use rostfrei_messaging_core::{
    BoundedContext, CausationId, ContractError, CorrelationId, EnvelopeContext,
    IntegrationEventAddress, IntegrationEventEnvelope, MessageId, MessageTimestamp,
    OutboundMessage, PublishReceipt, SchemaVersion,
};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::command_bus::{canonical_serialize, framed_fingerprint};

pub trait IntegrationEvent: Serialize + DeserializeOwned + Send + Sync + Sized + 'static {
    const EVENT_NAME: &'static str;
    const SCHEMA_VERSION: u32;
}

/// Maps a private committed domain event to its public integration event.
pub trait IntegrationEventMapper<D>: Send + Sync {
    type Output: IntegrationEvent;

    fn map(&self, event: &CommittedDomainEvent<'_, D>) -> Self::Output;
}

/// Publishes the integration event produced by an application mapper.
pub struct IntegrationEventPublisher<M> {
    bus: IntegrationEventBus,
    mapper: M,
}

impl<M> IntegrationEventPublisher<M> {
    pub const fn new(bus: IntegrationEventBus, mapper: M) -> Self {
        Self { bus, mapper }
    }
}

/// Adds typed domain-to-integration mappings without exposing dispatcher plumbing.
pub trait IntegrationEventDispatcherExt {
    fn register_integration_event<A, D, M>(
        &mut self,
        bus: IntegrationEventBus,
        mapper: M,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: Event + EventVariant<D> + Send + Sync + 'static,
        D: DomainEvent + Send + Sync,
        M: IntegrationEventMapper<D> + 'static;
}

impl IntegrationEventDispatcherExt for DomainEventDispatcher {
    fn register_integration_event<A, D, M>(
        &mut self,
        bus: IntegrationEventBus,
        mapper: M,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: Event + EventVariant<D> + Send + Sync + 'static,
        D: DomainEvent + Send + Sync,
        M: IntegrationEventMapper<D> + 'static,
    {
        self.register::<A, D, _>(
            D::LOCAL_ID,
            Arc::new(IntegrationEventPublisher::new(bus, mapper)),
        )
    }
}

#[async_trait]
impl<D, M> DomainEventHandler<D> for IntegrationEventPublisher<M>
where
    D: Send + Sync,
    M: IntegrationEventMapper<D>,
{
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, D>,
    ) -> Result<(), DomainEventHandlerError> {
        let committed = CommittedEventContext::new(event.recorded())
            .map_err(|error| classify_publication_error(&error))?;
        self.bus
            .publish(committed, self.mapper.map(event))
            .await
            .map(|_| ())
            .map_err(|error| classify_publication_error(&error))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedEventContext {
    source_event_id: EventId,
    correlation_id: CorrelationId,
    occurred_at: Option<MessageTimestamp>,
}

impl CommittedEventContext {
    pub fn new(recorded: &RecordedEvent) -> Result<Self, IntegrationEventBusError> {
        let correlation_id = recorded.correlation_id().cloned().ok_or_else(|| {
            IntegrationEventBusError::new(
                IntegrationEventBusErrorKind::InvalidContext,
                "a committed event must have a correlation ID before it can publish an integration event",
            )
        })?;
        Ok(Self {
            source_event_id: recorded.event_id().clone(),
            correlation_id,
            occurred_at: None,
        })
    }

    #[must_use]
    pub const fn with_occurred_at(mut self, occurred_at: MessageTimestamp) -> Self {
        self.occurred_at = Some(occurred_at);
        self
    }

    pub const fn source_event_id(&self) -> &EventId {
        &self.source_event_id
    }

    pub const fn correlation_id(&self) -> &CorrelationId {
        &self.correlation_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedIntegrationMessage {
    message: OutboundMessage<IntegrationEventAddress>,
}

impl EncodedIntegrationMessage {
    pub(crate) const fn new(message: OutboundMessage<IntegrationEventAddress>) -> Self {
        Self { message }
    }

    pub fn from_delivery(
        address: IntegrationEventAddress,
        message_id: MessageId,
        payload: Vec<u8>,
        correlation_id: Option<CorrelationId>,
    ) -> Result<Self, IntegrationEventBusError> {
        let mut message = OutboundMessage::new(address, message_id, payload)
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        if let Some(correlation_id) = correlation_id {
            message = message.with_correlation_id(correlation_id);
        }
        Ok(Self::new(message))
    }

    pub const fn message(&self) -> &OutboundMessage<IntegrationEventAddress> {
        &self.message
    }

    pub const fn address(&self) -> &IntegrationEventAddress {
        self.message.address()
    }

    pub const fn message_id(&self) -> &MessageId {
        self.message.message_id()
    }

    pub fn payload(&self) -> &[u8] {
        self.message.payload()
    }

    pub fn decode<E>(&self) -> Result<IntegrationEventEnvelope<E>, IntegrationEventBusError>
    where
        E: IntegrationEvent,
    {
        let envelope: IntegrationEventEnvelope<E> = serde_json::from_slice(self.payload())
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        let source_event_id = envelope
            .causation_id()
            .ok_or_else(|| invalid_integration_message("integration event causation is missing"))
            .and_then(|causation_id| {
                EventId::new(causation_id.as_str()).map_err(|error| {
                    invalid_integration_message(format!(
                        "integration event causation is invalid: {error}"
                    ))
                })
            })?;
        let expected_message_id =
            integration_message_id(self.address(), envelope.schema_version(), &source_event_id)?;
        if envelope.message_id() != self.message_id()
            || self.message_id() != &expected_message_id
            || self.message.correlation_id() != Some(envelope.correlation_id())
            || self.address().name() != E::EVENT_NAME
            || envelope.schema_version().get() != E::SCHEMA_VERSION
        {
            return Err(IntegrationEventBusError::new(
                IntegrationEventBusErrorKind::InvalidMessage,
                "integration event envelope identity, route, or schema is inconsistent",
            ));
        }
        Ok(envelope)
    }
}

fn invalid_integration_message(message: impl Into<String>) -> IntegrationEventBusError {
    IntegrationEventBusError::new(IntegrationEventBusErrorKind::InvalidMessage, message)
}

#[async_trait]
pub trait IntegrationMessageAdapter: Send + Sync {
    async fn publish(
        &self,
        message: EncodedIntegrationMessage,
    ) -> Result<PublishReceipt, IntegrationEventBusError>;
}

#[derive(Clone)]
pub struct IntegrationEventBus {
    context: BoundedContext,
    adapter: Arc<dyn IntegrationMessageAdapter>,
}

impl IntegrationEventBus {
    pub const fn new(context: BoundedContext, adapter: Arc<dyn IntegrationMessageAdapter>) -> Self {
        Self { context, adapter }
    }

    pub const fn context(&self) -> &BoundedContext {
        &self.context
    }

    pub async fn publish<E>(
        &self,
        committed: CommittedEventContext,
        event: E,
    ) -> Result<IntegrationEventPublication, IntegrationEventBusError>
    where
        E: IntegrationEvent,
    {
        let message = self.encode(committed, event)?;
        let message_id = message.message_id().clone();
        let receipt = self.adapter.publish(message).await?;
        Ok(IntegrationEventPublication::new(
            message_id,
            receipt.duplicate(),
        ))
    }

    pub fn encode<E>(
        &self,
        committed: CommittedEventContext,
        event: E,
    ) -> Result<EncodedIntegrationMessage, IntegrationEventBusError>
    where
        E: IntegrationEvent,
    {
        let address = self
            .context
            .integration_event_address(E::EVENT_NAME)
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        let schema_version = SchemaVersion::new(E::SCHEMA_VERSION)
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        let message_id =
            integration_message_id(&address, schema_version, committed.source_event_id())?;
        let causation_id = CausationId::new(committed.source_event_id().as_str())
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        let occurred_at = match committed.occurred_at {
            Some(occurred_at) => occurred_at,
            None => current_timestamp()?,
        };
        let correlation_id = committed.correlation_id;
        let envelope = IntegrationEventEnvelope::new(
            EnvelopeContext::new(
                message_id.clone(),
                schema_version,
                correlation_id.clone(),
                Some(causation_id),
            ),
            occurred_at,
            event,
        )
        .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        let payload = canonical_serialize(&envelope)
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?;
        let message = OutboundMessage::new(address, message_id, payload)
            .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))?
            .with_correlation_id(correlation_id);
        Ok(EncodedIntegrationMessage::new(message))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationEventPublication {
    message_id: MessageId,
    duplicate: bool,
}

impl IntegrationEventPublication {
    pub const fn new(message_id: MessageId, duplicate: bool) -> Self {
        Self {
            message_id,
            duplicate,
        }
    }

    pub const fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    pub const fn duplicate(&self) -> bool {
        self.duplicate
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum IntegrationEventBusErrorKind {
    #[error("integration event context is invalid")]
    InvalidContext,
    #[error("integration event encoding failed")]
    Encoding,
    #[error("integration event message is invalid")]
    InvalidMessage,
    #[error("integration event publication timed out")]
    Timeout,
    #[error("integration event messaging is unavailable")]
    Unavailable,
    #[error("integration event messaging configuration is invalid")]
    InvalidConfiguration,
    #[error("integration event publication was rejected")]
    Rejected,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{message}")]
pub struct IntegrationEventBusError {
    kind: IntegrationEventBusErrorKind,
    message: String,
}

impl IntegrationEventBusError {
    pub fn new(kind: IntegrationEventBusErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn encoding(message: impl Into<String>) -> Self {
        Self::new(IntegrationEventBusErrorKind::Encoding, message)
    }

    pub const fn kind(&self) -> IntegrationEventBusErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

fn classify_publication_error(error: &IntegrationEventBusError) -> DomainEventHandlerError {
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

pub fn integration_message_id(
    address: &IntegrationEventAddress,
    schema_version: SchemaVersion,
    source_event_id: &EventId,
) -> Result<MessageId, IntegrationEventBusError> {
    let schema_version = schema_version.get().to_be_bytes();
    let digest = framed_fingerprint(&[
        b"rostfrei:integration-event-message:v1",
        address.as_str().as_bytes(),
        &schema_version,
        source_event_id.as_str().as_bytes(),
    ]);
    MessageId::new(digest.to_hex())
        .map_err(|error: ContractError| IntegrationEventBusError::encoding(error.to_string()))
}

fn current_timestamp() -> Result<MessageTimestamp, IntegrationEventBusError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IntegrationEventBusError::encoding("system clock is before the Unix epoch"))?
        .as_millis();
    let milliseconds = u64::try_from(milliseconds).map_err(|_| {
        IntegrationEventBusError::encoding("system clock is outside the message timestamp range")
    })?;
    MessageTimestamp::from_unix_milliseconds(milliseconds)
        .map_err(|error| IntegrationEventBusError::encoding(error.to_string()))
}
