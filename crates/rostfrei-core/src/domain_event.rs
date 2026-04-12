use std::{any::TypeId, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    Aggregate, AggregateType, Event, EventCodec, EventCodecErrorKind, EventVariant, JsonEventCodec,
    RecordedEvent, MAX_EVENT_TYPE_LEN,
};

pub struct CommittedDomainEvent<'a, E> {
    recorded: &'a RecordedEvent,
    event: E,
}

impl<'a, E> CommittedDomainEvent<'a, E> {
    fn new(recorded: &'a RecordedEvent, event: E) -> Self {
        Self { recorded, event }
    }

    pub const fn recorded(&self) -> &RecordedEvent {
        self.recorded
    }

    pub const fn event(&self) -> &E {
        &self.event
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DomainEventHandlerErrorKind {
    Retryable,
    PermanentlyUnsupported,
    InvalidCommittedEvent,
    OperatorBlocking,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{kind:?}: {message}")]
pub struct DomainEventHandlerError {
    kind: DomainEventHandlerErrorKind,
    message: String,
}

impl DomainEventHandlerError {
    pub fn new(kind: DomainEventHandlerErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> DomainEventHandlerErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainEventDispatchOutcome {
    Handled,
    Ignored,
}

#[async_trait]
pub trait DomainEventHandler<E>: Send + Sync {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, E>,
    ) -> Result<(), DomainEventHandlerError>;
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DomainEventRegistrationError {
    #[error("registered aggregate type is invalid")]
    InvalidAggregateType,
    #[error("registered domain event type is invalid")]
    InvalidEventType,
    #[error(
        "a handler is already registered for aggregate `{aggregate_type}` event `{event_type}`"
    )]
    Conflict {
        aggregate_type: String,
        event_type: String,
    },
    #[error("aggregate `{aggregate_type}` was registered with a different model or codec")]
    AggregateConflict { aggregate_type: String },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RegistrationKey {
    aggregate_type: String,
    event_type: String,
}

#[async_trait]
trait ErasedDomainEventHandler: Send + Sync {
    async fn handle(
        &self,
        event: &RecordedEvent,
    ) -> Result<DomainEventDispatchOutcome, DomainEventHandlerError>;
}

struct TypedDomainEventHandler<A, E, C, H>
where
    A: Aggregate,
{
    codec: Arc<C>,
    handler: Arc<H>,
    marker: std::marker::PhantomData<(A, E)>,
}

#[async_trait]
impl<A, E, C, H> ErasedDomainEventHandler for TypedDomainEventHandler<A, E, C, H>
where
    A: Aggregate + Send + Sync + 'static,
    A::Event: EventVariant<E> + Send + Sync + 'static,
    E: Send + Sync + 'static,
    C: EventCodec<A> + 'static,
    H: DomainEventHandler<E> + 'static,
{
    async fn handle(
        &self,
        event: &RecordedEvent,
    ) -> Result<DomainEventDispatchOutcome, DomainEventHandlerError> {
        let decoded = self.codec.decode(event).map_err(|error| {
            let classification = match error.kind() {
                EventCodecErrorKind::UnknownEventType
                | EventCodecErrorKind::UnsupportedSchemaVersion
                | EventCodecErrorKind::MalformedPayload
                | EventCodecErrorKind::InvalidEnvelope => {
                    DomainEventHandlerErrorKind::InvalidCommittedEvent
                }
                EventCodecErrorKind::EncodingFailed => {
                    DomainEventHandlerErrorKind::OperatorBlocking
                }
            };
            DomainEventHandlerError::new(classification, error.to_string())
        })?;
        let concrete = decoded.into_event().ok_or_else(|| {
            DomainEventHandlerError::new(
                DomainEventHandlerErrorKind::InvalidCommittedEvent,
                "event codec decoded a different event variant than the registered handler",
            )
        })?;
        self.handler
            .handle(&CommittedDomainEvent::new(event, concrete))
            .await?;
        Ok(DomainEventDispatchOutcome::Handled)
    }
}

#[derive(Default)]
pub struct DomainEventDispatcher {
    handlers: HashMap<RegistrationKey, Arc<dyn ErasedDomainEventHandler>>,
    aggregate_registrations: HashMap<String, (TypeId, TypeId)>,
}

impl DomainEventDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<A, E, H>(
        &mut self,
        event_type: impl Into<String>,
        handler: Arc<H>,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: Event + EventVariant<E> + Send + Sync + 'static,
        E: Send + Sync + 'static,
        H: DomainEventHandler<E> + 'static,
    {
        self.register_with_codec::<A, E, JsonEventCodec, H>(
            event_type,
            Arc::new(JsonEventCodec),
            handler,
        )
    }

    pub fn register_with_codec<A, E, C, H>(
        &mut self,
        event_type: impl Into<String>,
        codec: Arc<C>,
        handler: Arc<H>,
    ) -> Result<(), DomainEventRegistrationError>
    where
        A: Aggregate + Send + Sync + 'static,
        A::Event: EventVariant<E> + Send + Sync + 'static,
        E: Send + Sync + 'static,
        C: EventCodec<A> + 'static,
        H: DomainEventHandler<E> + 'static,
    {
        AggregateType::new(A::AGGREGATE_TYPE)
            .map_err(|_| DomainEventRegistrationError::InvalidAggregateType)?;
        let event_type = event_type.into();
        if event_type.is_empty()
            || event_type.len() > MAX_EVENT_TYPE_LEN
            || event_type.trim() != event_type
            || event_type.chars().any(char::is_control)
        {
            return Err(DomainEventRegistrationError::InvalidEventType);
        }
        let key = RegistrationKey {
            aggregate_type: A::AGGREGATE_TYPE.to_owned(),
            event_type,
        };
        if self.handlers.contains_key(&key) {
            return Err(DomainEventRegistrationError::Conflict {
                aggregate_type: key.aggregate_type,
                event_type: key.event_type,
            });
        }
        let aggregate_registration = (TypeId::of::<A>(), TypeId::of::<C>());
        if self
            .aggregate_registrations
            .get(&key.aggregate_type)
            .is_some_and(|registered| registered != &aggregate_registration)
        {
            return Err(DomainEventRegistrationError::AggregateConflict {
                aggregate_type: key.aggregate_type,
            });
        }
        self.aggregate_registrations
            .insert(key.aggregate_type.clone(), aggregate_registration);
        self.handlers.insert(
            key,
            Arc::new(TypedDomainEventHandler::<A, E, C, H> {
                codec,
                handler,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(())
    }

    pub async fn dispatch(
        &self,
        event: &RecordedEvent,
    ) -> Result<DomainEventDispatchOutcome, DomainEventHandlerError> {
        let key = RegistrationKey {
            aggregate_type: event.stream_id().aggregate_type().as_str().to_owned(),
            event_type: event.event_type().to_owned(),
        };
        let Some(handler) = self.handlers.get(&key) else {
            return Ok(DomainEventDispatchOutcome::Ignored);
        };
        handler.handle(event).await
    }
}
