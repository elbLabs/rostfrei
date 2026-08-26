use thiserror::Error;

use crate::{EventId, NewEvent, RecordedEvent};

pub trait Aggregate: Sized {
    type Event;

    const AGGREGATE_TYPE: &'static str;

    fn initial() -> Self;

    fn apply(&mut self, event: &Self::Event);
}

pub struct DecisionContext<'a, A: Aggregate> {
    state: &'a mut A,
    recorded: &'a mut Vec<A::Event>,
}

impl<'a, A: Aggregate> DecisionContext<'a, A> {
    pub fn new(state: &'a mut A, recorded: &'a mut Vec<A::Event>) -> Self {
        Self { state, recorded }
    }

    pub fn state(&self) -> &A {
        self.state
    }

    pub fn record(&mut self, event: A::Event) {
        self.state.apply(&event);
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
