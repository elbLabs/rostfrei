use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::{
    AppendOutcome, CommitId, EventBatch, EventHistory, EventId, EventStore, EventStoreError,
    EventStoreErrorKind, ExpectedVersion, OperationId, RecordedEvent, StreamId, StreamVersion,
};

#[derive(Clone)]
struct StoredAppend {
    batch: EventBatch,
    events: Vec<RecordedEvent>,
}

#[derive(Default)]
struct State {
    streams: HashMap<StreamId, Vec<RecordedEvent>>,
    operations: HashMap<(StreamId, OperationId), StoredAppend>,
    commits: HashMap<(StreamId, CommitId), OperationId>,
    event_ids: HashSet<(StreamId, EventId)>,
    event_count: usize,
}

#[derive(Clone)]
pub struct InMemoryEventStore {
    state: Arc<Mutex<State>>,
    maximum_events: usize,
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            maximum_events: usize::MAX,
        }
    }

    pub fn with_capacity(maximum_events: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            maximum_events,
        }
    }

    pub async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        <Self as EventHistory>::load(self, stream_id).await
    }
}

#[async_trait]
impl EventHistory for InMemoryEventStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        let state = self.state.lock().await;
        Ok(state.streams.get(stream_id).cloned().unwrap_or_default())
    }
}

#[async_trait]
#[allow(clippy::too_many_lines)]
impl EventStore for InMemoryEventStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        let mut state = self.state.lock().await;

        let operation_key = (stream_id.clone(), batch.operation_id().clone());
        if let Some(previous) = state.operations.get(&operation_key) {
            if previous.batch == batch {
                return Ok(AppendOutcome::ExactReplay(previous.events.clone()));
            }
            return Err(identity_conflict(
                "operation identity was reused with different content",
            ));
        }
        let commit_key = (stream_id.clone(), batch.commit_id().clone());
        if state.commits.contains_key(&commit_key) {
            return Err(identity_conflict(
                "commit identity was reused with different content",
            ));
        }
        if batch.events().iter().any(|event| {
            state
                .event_ids
                .contains(&(stream_id.clone(), event.event_id().clone()))
        }) {
            return Err(identity_conflict(
                "event identity was reused with different content",
            ));
        }

        validate_derived_identities(stream_id, &batch)?;

        let current_version = state
            .streams
            .get(stream_id)
            .and_then(|events| events.last())
            .map_or(StreamVersion::ZERO, RecordedEvent::stream_version);
        match expected_version {
            ExpectedVersion::NoStream if current_version == StreamVersion::ZERO => {}
            ExpectedVersion::Exact(version) if version == StreamVersion::ZERO => {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::InvalidRequest,
                    "Exact requires a non-zero stream version; use NoStream for an absent stream",
                ));
            }
            ExpectedVersion::Exact(version) if version == current_version => {}
            ExpectedVersion::NoStream | ExpectedVersion::Exact(_) => {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::Conflict,
                    format!("expected version does not match current version {current_version:?}"),
                ));
            }
        }

        let new_count = state
            .event_count
            .checked_add(batch.events().len())
            .filter(|count| *count <= self.maximum_events)
            .ok_or_else(|| {
                EventStoreError::new(
                    EventStoreErrorKind::CapacityExhausted,
                    "in-memory event capacity would be exceeded",
                )
            })?;

        let mut next_version = current_version;
        let mut recorded = Vec::with_capacity(batch.events().len());
        let event_count = u32::try_from(batch.events().len()).map_err(|_| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "event count exceeds the supported range",
            )
        })?;
        for (ordinal, event) in batch.events().iter().cloned().enumerate() {
            next_version = next_version.next().ok_or_else(|| {
                EventStoreError::new(
                    EventStoreErrorKind::CapacityExhausted,
                    "stream version space is exhausted",
                )
            })?;
            recorded.push(RecordedEvent::from_new(
                stream_id.clone(),
                next_version,
                &batch,
                u32::try_from(ordinal).map_err(|_| {
                    EventStoreError::new(
                        EventStoreErrorKind::InvalidRequest,
                        "event ordinal exceeds the supported range",
                    )
                })?,
                event_count,
                event,
            ));
        }

        let stored = StoredAppend {
            batch: batch.clone(),
            events: recorded.clone(),
        };
        state
            .commits
            .insert(commit_key, batch.operation_id().clone());
        state.event_ids.extend(
            batch
                .events()
                .iter()
                .map(|event| (stream_id.clone(), event.event_id().clone())),
        );
        state.operations.insert(operation_key, stored);
        state
            .streams
            .entry(stream_id.clone())
            .or_default()
            .extend(recorded.iter().cloned());
        state.event_count = new_count;

        Ok(AppendOutcome::Appended(recorded))
    }
}

fn validate_derived_identities(
    stream_id: &StreamId,
    batch: &EventBatch,
) -> Result<(), EventStoreError> {
    let expected_commit = derive_commit_id(stream_id, batch.operation_id());
    if batch.commit_id() != &expected_commit {
        return Err(EventStoreError::new(
            EventStoreErrorKind::InvalidRequest,
            "commit identity was not derived from the stream and operation identity",
        ));
    }
    for (ordinal, event) in batch.events().iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "event ordinal exceeds the supported range",
            )
        })?;
        if event.event_id() != &derive_event_id(batch.commit_id(), ordinal) {
            return Err(EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "event identity was not derived from its commit identity and ordinal",
            ));
        }
    }
    Ok(())
}

fn identity_conflict(message: &'static str) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::IdentityConflict, message)
}
