use std::collections::HashSet;

use rostfrei_messaging_core::{CausationId, CorrelationId, MessageTimestamp};
use thiserror::Error;

use crate::{CommitId, ContentFingerprint, EventId, OperationId, StreamId};

pub const MAX_EVENT_TYPE_LEN: usize = 128;
pub const MAX_EVENT_PAYLOAD_LEN: usize = 1024 * 1024;
pub const MAX_BATCH_PAYLOAD_LEN: usize = 1024 * 1024;
pub const MAX_EVENTS_PER_BATCH: usize = 100;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StreamVersion(u64);

impl StreamVersion {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedVersion {
    NoStream,
    Exact(StreamVersion),
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum EnvelopeError {
    #[error("event type must not be empty")]
    EmptyEventType,
    #[error("event type exceeds its {maximum}-byte limit")]
    EventTypeTooLong { maximum: usize },
    #[error("event type must not have leading or trailing whitespace or control characters")]
    InvalidEventType,
    #[error("schema version must be greater than zero")]
    InvalidSchemaVersion,
    #[error("event payload exceeds its {maximum}-byte limit")]
    PayloadTooLarge { maximum: usize },
    #[error("an event batch must contain at least one event")]
    EmptyBatch,
    #[error(
        "event batch contains {actual} domain events, exceeding the {maximum}-event atomic commit limit; split the work across commands"
    )]
    BatchTooLarge { actual: usize, maximum: usize },
    #[error("an event batch payload exceeds its {maximum}-byte limit")]
    BatchPayloadTooLarge { maximum: usize },
    #[error("an event batch must not contain duplicate event identities")]
    DuplicateEventId,
    #[error("a recorded event must have a non-zero stream version")]
    ZeroRecordedVersion,
    #[error("a recorded event has invalid commit coordinates")]
    InvalidCommitCoordinates,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewEvent {
    event_id: EventId,
    event_type: String,
    schema_version: u32,
    payload: Vec<u8>,
}

impl NewEvent {
    pub fn new(
        event_id: EventId,
        event_type: impl Into<String>,
        schema_version: u32,
        payload: impl Into<Vec<u8>>,
    ) -> Result<Self, EnvelopeError> {
        let event_type = event_type.into();
        validate_event_type(&event_type)?;
        if schema_version == 0 {
            return Err(EnvelopeError::InvalidSchemaVersion);
        }
        let payload = payload.into();
        if payload.len() > MAX_EVENT_PAYLOAD_LEN {
            return Err(EnvelopeError::PayloadTooLarge {
                maximum: MAX_EVENT_PAYLOAD_LEN,
            });
        }
        Ok(Self {
            event_id,
            event_type,
            schema_version,
            payload,
        })
    }

    pub const fn event_id(&self) -> &EventId {
        &self.event_id
    }

    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventBatch {
    commit_id: CommitId,
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    events: Vec<NewEvent>,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
}

impl EventBatch {
    pub fn new(
        commit_id: CommitId,
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
        events: Vec<NewEvent>,
    ) -> Result<Self, EnvelopeError> {
        if events.is_empty() {
            return Err(EnvelopeError::EmptyBatch);
        }
        if events.len() > MAX_EVENTS_PER_BATCH {
            return Err(EnvelopeError::BatchTooLarge {
                actual: events.len(),
                maximum: MAX_EVENTS_PER_BATCH,
            });
        }
        let payload_bytes = events.iter().try_fold(0_usize, |total, event| {
            total.checked_add(event.payload().len())
        });
        if payload_bytes.is_none_or(|total| total > MAX_BATCH_PAYLOAD_LEN) {
            return Err(EnvelopeError::BatchPayloadTooLarge {
                maximum: MAX_BATCH_PAYLOAD_LEN,
            });
        }
        let unique_ids: HashSet<_> = events.iter().map(NewEvent::event_id).collect();
        if unique_ids.len() != events.len() {
            return Err(EnvelopeError::DuplicateEventId);
        }
        Ok(Self {
            commit_id,
            operation_id,
            operation_fingerprint,
            events,
            correlation_id: None,
            causation_id: None,
        })
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    pub const fn commit_id(&self) -> &CommitId {
        &self.commit_id
    }

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn operation_fingerprint(&self) -> ContentFingerprint {
        self.operation_fingerprint
    }

    pub fn events(&self) -> &[NewEvent] {
        &self.events
    }

    pub const fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub fn into_events(self) -> Vec<NewEvent> {
        self.events
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedEvent {
    stream_id: StreamId,
    stream_version: StreamVersion,
    event_id: EventId,
    commit_id: CommitId,
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    commit_event_ordinal: u32,
    commit_event_count: u32,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    committed_at: Option<MessageTimestamp>,
    event_type: String,
    schema_version: u32,
    payload: Vec<u8>,
}

impl RecordedEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stream_id: StreamId,
        stream_version: StreamVersion,
        event_id: EventId,
        commit_id: CommitId,
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
        event_type: impl Into<String>,
        schema_version: u32,
        payload: impl Into<Vec<u8>>,
    ) -> Result<Self, EnvelopeError> {
        Self::new_in_commit(
            stream_id,
            stream_version,
            event_id,
            commit_id,
            operation_id,
            operation_fingerprint,
            0,
            1,
            event_type,
            schema_version,
            payload,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_in_commit(
        stream_id: StreamId,
        stream_version: StreamVersion,
        event_id: EventId,
        commit_id: CommitId,
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
        commit_event_ordinal: u32,
        commit_event_count: u32,
        event_type: impl Into<String>,
        schema_version: u32,
        payload: impl Into<Vec<u8>>,
    ) -> Result<Self, EnvelopeError> {
        if stream_version == StreamVersion::ZERO {
            return Err(EnvelopeError::ZeroRecordedVersion);
        }
        if commit_event_count == 0
            || u32::try_from(MAX_EVENTS_PER_BATCH).is_ok_and(|maximum| commit_event_count > maximum)
            || commit_event_ordinal >= commit_event_count
        {
            return Err(EnvelopeError::InvalidCommitCoordinates);
        }
        let event_type = event_type.into();
        validate_event_type(&event_type)?;
        if schema_version == 0 {
            return Err(EnvelopeError::InvalidSchemaVersion);
        }
        let payload = payload.into();
        if payload.len() > MAX_EVENT_PAYLOAD_LEN {
            return Err(EnvelopeError::PayloadTooLarge {
                maximum: MAX_EVENT_PAYLOAD_LEN,
            });
        }
        Ok(Self {
            stream_id,
            stream_version,
            event_id,
            commit_id,
            operation_id,
            operation_fingerprint,
            commit_event_ordinal,
            commit_event_count,
            correlation_id: None,
            causation_id: None,
            committed_at: None,
            event_type,
            schema_version,
            payload,
        })
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: CorrelationId) -> Self {
        self.correlation_id = Some(correlation_id);
        self
    }

    #[must_use]
    pub fn with_causation_id(mut self, causation_id: CausationId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    #[must_use]
    pub const fn with_committed_at(mut self, committed_at: MessageTimestamp) -> Self {
        self.committed_at = Some(committed_at);
        self
    }

    pub const fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub const fn stream_version(&self) -> StreamVersion {
        self.stream_version
    }

    pub const fn event_id(&self) -> &EventId {
        &self.event_id
    }

    pub const fn commit_id(&self) -> &CommitId {
        &self.commit_id
    }

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn operation_fingerprint(&self) -> ContentFingerprint {
        self.operation_fingerprint
    }

    pub const fn commit_event_ordinal(&self) -> u32 {
        self.commit_event_ordinal
    }

    pub const fn commit_event_count(&self) -> u32 {
        self.commit_event_count
    }

    pub const fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub const fn committed_at(&self) -> Option<MessageTimestamp> {
        self.committed_at
    }

    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub(crate) fn from_new(
        stream_id: StreamId,
        stream_version: StreamVersion,
        batch: &EventBatch,
        commit_event_ordinal: u32,
        commit_event_count: u32,
        event: NewEvent,
    ) -> Self {
        Self {
            stream_id,
            stream_version,
            event_id: event.event_id,
            commit_id: batch.commit_id.clone(),
            operation_id: batch.operation_id.clone(),
            operation_fingerprint: batch.operation_fingerprint,
            commit_event_ordinal,
            commit_event_count,
            correlation_id: batch.correlation_id.clone(),
            causation_id: batch.causation_id.clone(),
            committed_at: None,
            event_type: event.event_type,
            schema_version: event.schema_version,
            payload: event.payload,
        }
    }
}

fn validate_event_type(event_type: &str) -> Result<(), EnvelopeError> {
    if event_type.is_empty() {
        return Err(EnvelopeError::EmptyEventType);
    }
    if event_type.len() > MAX_EVENT_TYPE_LEN {
        return Err(EnvelopeError::EventTypeTooLong {
            maximum: MAX_EVENT_TYPE_LEN,
        });
    }
    if event_type.trim() != event_type || event_type.chars().any(char::is_control) {
        return Err(EnvelopeError::InvalidEventType);
    }
    Ok(())
}
