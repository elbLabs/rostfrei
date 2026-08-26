use std::borrow::Cow;

use thiserror::Error;

use crate::{EventId, NewEvent, RecordedEvent, StreamId};

pub trait Aggregate: Sized {
    type State;
    type Event;

    const AGGREGATE_TYPE: &'static str;

    fn aggregate_type() -> Cow<'static, str> {
        Cow::Borrowed(Self::AGGREGATE_TYPE)
    }

    fn initial(stream_id: &StreamId) -> Self::State;

    fn apply(state: &mut Self::State, event: &Self::Event);
}

pub trait Event: Sized {
    fn event_type(&self) -> &'static str;

    fn schema_version(&self) -> u32;

    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError>;

    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError>;
}

pub trait EventVariant<E>: Sized {
    fn event(&self) -> Option<&E>;

    fn into_event(self) -> Option<E>;
}

impl<E> EventVariant<E> for E {
    fn event(&self) -> Option<&E> {
        Some(self)
    }

    fn into_event(self) -> Option<E> {
        Some(self)
    }
}

pub struct DecisionContext<'a, A: Aggregate> {
    state: &'a mut A::State,
    recorded: &'a mut Vec<A::Event>,
}

impl<'a, A: Aggregate> DecisionContext<'a, A> {
    pub fn new(state: &'a mut A::State, recorded: &'a mut Vec<A::Event>) -> Self {
        Self { state, recorded }
    }

    pub fn state(&self) -> &A::State {
        self.state
    }

    pub fn record<E>(&mut self, event: E)
    where
        E: Into<A::Event>,
    {
        let event = event.into();
        A::apply(self.state, &event);
        self.recorded.push(event);
    }

    pub fn recorded(&self) -> &[A::Event] {
        self.recorded
    }
}

pub trait CommandHandler<Command>: Aggregate {
    type Rejection;

    fn handle(
        command: &Command,
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum EventCodecErrorKind {
    UnknownEventType,
    UnsupportedSchemaVersion,
    MalformedPayload,
    InvalidEnvelope,
    EncodingFailed,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{kind:?}: {message}")]
pub struct EventCodecError {
    kind: EventCodecErrorKind,
    message: String,
}

impl EventCodecError {
    pub fn new(kind: EventCodecErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> EventCodecErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub trait EventCodec<A: Aggregate>: Send + Sync {
    fn encode(&self, event: &A::Event, event_id: EventId) -> Result<NewEvent, EventCodecError>;

    fn decode(&self, event: &RecordedEvent) -> Result<A::Event, EventCodecError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct JsonEventCodec;

impl<A> EventCodec<A> for JsonEventCodec
where
    A: Aggregate,
    A::Event: Event,
{
    fn encode(&self, event: &A::Event, event_id: EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(
            event_id,
            event.event_type(),
            event.schema_version(),
            event.encode_json()?,
        )
        .map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, error.to_string())
        })
    }

    fn decode(&self, event: &RecordedEvent) -> Result<A::Event, EventCodecError> {
        A::Event::decode_json(event)
    }
}
