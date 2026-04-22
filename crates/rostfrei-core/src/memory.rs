use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::store::validate_transaction_item_limit;
use crate::{
    AppendOutcome, CommitId, EventBatch, EventHistory, EventId, EventStore, EventStoreError,
    EventStoreErrorKind, EventTransaction, ExpectedVersion, OperationId, RecordedEvent, StreamId,
    StreamVersion, TransactionAppendOutcome, TransactionReceipt, TransactionStreamReceipt,
};

#[derive(Clone)]
struct StoredAppend {
    batch: EventBatch,
    events: Vec<RecordedEvent>,
    provenance: AppendProvenance,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AppendProvenance {
    Direct,
    Transaction,
}

#[derive(Clone)]
struct StoredTransaction {
    transaction: EventTransaction,
    receipt: TransactionReceipt,
}

#[derive(Default)]
struct State {
    streams: HashMap<StreamId, Vec<RecordedEvent>>,
    operations: HashMap<(StreamId, OperationId), StoredAppend>,
    commits: HashMap<(StreamId, CommitId), OperationId>,
    event_ids: HashSet<(StreamId, EventId)>,
    transactions: HashMap<(StreamId, OperationId), StoredTransaction>,
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
            if previous.provenance == AppendProvenance::Direct && previous.batch == batch {
                return Ok(AppendOutcome::ExactReplay(previous.events.clone()));
            }
            if previous.provenance == AppendProvenance::Transaction {
                return Err(identity_conflict(
                    "operation identity was already used by an event transaction",
                ));
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
            provenance: AppendProvenance::Direct,
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
        drop(state);
        Ok(AppendOutcome::Appended(recorded))
    }

    async fn load_transaction_receipt(
        &self,
        primary_stream_id: &StreamId,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        let state = self.state.lock().await;
        Ok(state
            .transactions
            .get(&(primary_stream_id.clone(), operation_id.clone()))
            .map(|stored| stored.receipt.clone()))
    }

    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        let total_events = validate_transaction_item_limit(&transaction)?;
        let primary_stream_id = transaction
            .primary_stream_id()
            .ok_or_else(|| invalid("an event transaction must contain at least one participant"))?
            .clone();
        let mut state = self.state.lock().await;
        let transaction_key = (primary_stream_id, transaction.operation_id().clone());
        if let Some(previous) = state.transactions.get(&transaction_key) {
            if transaction_content_matches(&previous.transaction, &transaction) {
                return Ok(TransactionAppendOutcome::ExactReplay(
                    previous.receipt.clone(),
                ));
            }
            return Err(identity_conflict(
                "transaction identity was reused with different content",
            ));
        }
        validate_transaction(&state, &transaction)?;

        let new_count = state
            .event_count
            .checked_add(total_events)
            .filter(|count| *count <= self.maximum_events)
            .ok_or_else(|| {
                EventStoreError::new(
                    EventStoreErrorKind::CapacityExhausted,
                    "in-memory event capacity would be exceeded",
                )
            })?;

        let mut staged = Vec::with_capacity(transaction.participants().len());
        for participant in transaction.participants() {
            let base_version = current_version(&state, participant.stream_id());
            let recorded = participant
                .batch()
                .map(|batch| record_batch(participant.stream_id(), base_version, batch))
                .transpose()?;
            staged.push((
                participant.clone(),
                base_version,
                recorded.unwrap_or_default(),
            ));
        }

        let mut receipt = TransactionReceipt::new(
            transaction.operation_id().clone(),
            transaction.operation_fingerprint(),
            staged
                .iter()
                .map(|(participant, base_version, events)| {
                    TransactionStreamReceipt::new(
                        participant.stream_id().clone(),
                        *base_version,
                        events.clone(),
                    )
                })
                .collect(),
        );
        if let Some(correlation_id) = transaction.correlation_id() {
            receipt = receipt.with_correlation_id(correlation_id.clone());
        }
        if let Some(causation_id) = transaction.causation_id() {
            receipt = receipt.with_causation_id(causation_id.clone());
        }

        for (participant, _, events) in staged {
            let Some(batch) = participant.batch() else {
                continue;
            };
            let stream_id = participant.stream_id().clone();
            state.commits.insert(
                (stream_id.clone(), batch.commit_id().clone()),
                batch.operation_id().clone(),
            );
            state.event_ids.extend(
                batch
                    .events()
                    .iter()
                    .map(|event| (stream_id.clone(), event.event_id().clone())),
            );
            state.operations.insert(
                (stream_id.clone(), batch.operation_id().clone()),
                StoredAppend {
                    batch: batch.clone(),
                    events: events.clone(),
                    provenance: AppendProvenance::Transaction,
                },
            );
            state.streams.entry(stream_id).or_default().extend(events);
        }
        state.event_count = new_count;
        state.transactions.insert(
            transaction_key,
            StoredTransaction {
                transaction,
                receipt: receipt.clone(),
            },
        );
        drop(state);
        Ok(TransactionAppendOutcome::Appended(receipt))
    }
}

fn transaction_content_matches(first: &EventTransaction, second: &EventTransaction) -> bool {
    first.operation_id() == second.operation_id()
        && first.operation_fingerprint() == second.operation_fingerprint()
        && first.correlation_id() == second.correlation_id()
        && first.causation_id() == second.causation_id()
        && first.participants().len() == second.participants().len()
        && first
            .participants()
            .iter()
            .zip(second.participants())
            .all(|(first, second)| {
                first.stream_id() == second.stream_id() && first.batch() == second.batch()
            })
}

fn validate_transaction(
    state: &State,
    transaction: &EventTransaction,
) -> Result<(), EventStoreError> {
    if transaction.participants().is_empty() {
        return Err(invalid(
            "an event transaction must contain at least one participant",
        ));
    }
    let mut streams = HashSet::with_capacity(transaction.participants().len());
    for participant in transaction.participants() {
        if !streams.insert(participant.stream_id()) {
            return Err(invalid(
                "an event transaction must not contain duplicate streams",
            ));
        }
        let Some(batch) = participant.batch() else {
            continue;
        };
        if batch.operation_id() != transaction.operation_id()
            || batch.operation_fingerprint() != transaction.operation_fingerprint()
            || batch.correlation_id() != transaction.correlation_id()
            || batch.causation_id() != transaction.causation_id()
        {
            return Err(invalid(
                "participant commit metadata does not match its transaction",
            ));
        }
        validate_derived_identities(participant.stream_id(), batch)?;
        if let Some(previous) = state.operations.get(&(
            participant.stream_id().clone(),
            batch.operation_id().clone(),
        )) {
            let message = match previous.provenance {
                AppendProvenance::Direct => {
                    "operation identity was already used outside an event transaction"
                }
                AppendProvenance::Transaction => {
                    "operation identity was already used by another event transaction"
                }
            };
            return Err(identity_conflict(message));
        }
        if state
            .commits
            .contains_key(&(participant.stream_id().clone(), batch.commit_id().clone()))
        {
            return Err(identity_conflict(
                "commit identity was reused with different content",
            ));
        }
        if batch.events().iter().any(|event| {
            state
                .event_ids
                .contains(&(participant.stream_id().clone(), event.event_id().clone()))
        }) {
            return Err(identity_conflict(
                "event identity was reused with different content",
            ));
        }
    }
    for participant in transaction.participants() {
        validate_expected_version(
            participant.expected_version(),
            current_version(state, participant.stream_id()),
        )?;
    }
    Ok(())
}

fn validate_expected_version(
    expected: ExpectedVersion,
    current: StreamVersion,
) -> Result<(), EventStoreError> {
    match expected {
        ExpectedVersion::NoStream if current == StreamVersion::ZERO => Ok(()),
        ExpectedVersion::Exact(version) if version == StreamVersion::ZERO => Err(invalid(
            "Exact requires a non-zero stream version; use NoStream for an absent stream",
        )),
        ExpectedVersion::Exact(version) if version == current => Ok(()),
        ExpectedVersion::NoStream | ExpectedVersion::Exact(_) => Err(EventStoreError::new(
            EventStoreErrorKind::Conflict,
            format!("expected version does not match current version {current:?}"),
        )),
    }
}

fn current_version(state: &State, stream_id: &StreamId) -> StreamVersion {
    state
        .streams
        .get(stream_id)
        .and_then(|events| events.last())
        .map_or(StreamVersion::ZERO, RecordedEvent::stream_version)
}

fn record_batch(
    stream_id: &StreamId,
    current_version: StreamVersion,
    batch: &EventBatch,
) -> Result<Vec<RecordedEvent>, EventStoreError> {
    let mut next_version = current_version;
    let mut recorded = Vec::with_capacity(batch.events().len());
    let event_count = u32::try_from(batch.events().len())
        .map_err(|_| invalid("event count exceeds the supported range"))?;
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
            batch,
            u32::try_from(ordinal)
                .map_err(|_| invalid("event ordinal exceeds the supported range"))?,
            event_count,
            event,
        ));
    }
    Ok(recorded)
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

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::new(EventStoreErrorKind::InvalidRequest, message)
}
