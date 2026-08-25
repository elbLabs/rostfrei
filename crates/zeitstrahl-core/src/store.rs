use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::{EventBatch, ExpectedVersion, RecordedEvent, StreamId};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EventStoreErrorKind {
    InvalidRequest,
    Conflict,
    IdentityConflict,
    CorruptHistory,
    CapacityExhausted,
    ConfigurationMismatch,
    Unavailable,
}

impl std::fmt::Display for EventStoreErrorKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{kind}: {message}")]
pub struct EventStoreError {
    kind: EventStoreErrorKind,
    message: String,
}

impl EventStoreError {
    pub fn new(kind: EventStoreErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> EventStoreErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AppendOutcome {
    Appended(Vec<RecordedEvent>),
    ExactReplay(Vec<RecordedEvent>),
}

impl AppendOutcome {
    pub fn events(&self) -> &[RecordedEvent] {
        match self {
            Self::Appended(events) | Self::ExactReplay(events) => events,
        }
    }

    pub fn into_events(self) -> Vec<RecordedEvent> {
        match self {
            Self::Appended(events) | Self::ExactReplay(events) => events,
        }
    }

    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
}

#[async_trait]
pub trait EventStore: Send + Sync {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError>;

    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError>;
}

#[async_trait]
impl<Store: EventStore + ?Sized> EventStore for Arc<Store> {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.as_ref().load(stream_id).await
    }

    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.as_ref()
            .append(stream_id, expected_version, batch)
            .await
    }
}
