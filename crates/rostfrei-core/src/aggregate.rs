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

pub struct AggregateInstance<A: Aggregate> {
    stream_id: StreamId,
    state: A::State,
    uncommitted_events: Vec<A::Event>,
}

impl<A: Aggregate> AggregateInstance<A> {
    pub fn new(stream_id: StreamId) -> Self {
        let state = A::initial(&stream_id);
        Self {
            stream_id,
            state,
            uncommitted_events: Vec::new(),
        }
    }

    pub fn rehydrate(stream_id: StreamId, events: impl IntoIterator<Item = A::Event>) -> Self {
        let mut aggregate = Self::new(stream_id);
        for event in events {
            A::apply(&mut aggregate.state, &event);
        }
        aggregate
    }

    pub const fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub const fn state(&self) -> &A::State {
        &self.state
    }

    pub fn raise<E>(&mut self, event: E)
    where
        E: Into<A::Event>,
    {
        let event = event.into();
        A::apply(&mut self.state, &event);
        self.uncommitted_events.push(event);
    }

    pub fn uncommitted_events(&self) -> &[A::Event] {
        &self.uncommitted_events
    }

    pub fn into_parts(self) -> (StreamId, A::State, Vec<A::Event>) {
        (self.stream_id, self.state, self.uncommitted_events)
    }
}

pub trait CommandHandler<Command>: Aggregate {
    type Rejection;

    fn handle(
        command: &Command,
        aggregate: &mut AggregateInstance<Self>,
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
