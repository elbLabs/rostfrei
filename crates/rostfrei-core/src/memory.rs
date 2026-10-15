use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_messaging_core::BoundedContextName;
use tokio::sync::Mutex;

use crate::identity::{derive_commit_id, derive_event_id};
use crate::store::validate_transaction_item_limit;
use crate::{
    AggregateType, AppendOutcome, CommitId, EventBatch, EventHistory, EventId, EventStore,
    EventStoreError, EventStoreErrorKind, EventTransaction, ExpectedVersion, OperationId,
    RecordedEvent, StreamDirectory, StreamId, StreamSummary, StreamVersion,
    TransactionAppendOutcome, TransactionReceipt, TransactionStreamReceipt,
};
use crate::{
    CommandAuthority, CommandBackend, CommandCommitSession, CompletedCommit, CompletedOutcome,
    DetailRead, DetailReadError, EncodedOutcomeDetails, OutcomeDecision, OutcomeDetailReader,
    OutcomeDetailReference, OutcomeDetails, OutcomeKey, OutcomeLifecycle, OutcomeLifecycleError,
    OutcomeLookup, OutcomeMarker, OutcomeReader, OutcomeRevision, ProposalDigest,
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
enum StoredTransaction {
    Scoped(Box<OutcomeMarker>),
    Unscoped {
        proposal: ProposalDigest,
        receipt: Box<TransactionReceipt>,
    },
}

impl StoredTransaction {
    const fn proposal(&self) -> ProposalDigest {
        match self {
            Self::Scoped(marker) => marker.proposal_digest(),
            Self::Unscoped { proposal, .. } => *proposal,
        }
    }
    fn acceptance(&self) -> Result<TransactionReceipt, EventStoreError> {
        match self {
            Self::Scoped(marker) if marker.decision() == OutcomeDecision::Rejected => Err(
                identity_conflict("operation has a completed rejection, not acceptance"),
            ),
            Self::Scoped(marker) => Ok(marker.integrity_receipt().clone()),
            Self::Unscoped { receipt, .. } => Ok(receipt.as_ref().clone()),
        }
    }
}

enum DetailState {
    Active(Vec<u8>),
    Expired,
}

#[derive(Default)]
struct State {
    streams: HashMap<StreamId, Vec<RecordedEvent>>,
    operations: HashMap<(StreamId, OperationId), StoredAppend>,
    commits: HashMap<(StreamId, CommitId), OperationId>,
    event_ids: HashSet<(StreamId, EventId)>,
    transactions: HashMap<(Option<BoundedContextName>, OperationId), StoredTransaction>,
    details: HashMap<OutcomeDetailReference, DetailState>,
    event_count: usize,
}

#[derive(Clone)]
pub struct InMemoryEventStore {
    state: Arc<Mutex<State>>,
    authority: CommandAuthority,
    hot: Arc<dyn OutcomeDetailReader>,
    cold: Option<Arc<dyn OutcomeDetailReader>>,
    maximum_events: usize,
    maximum_outcomes: usize,
}

impl Default for InMemoryEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let incarnation = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = Arc::new(Mutex::new(State::default()));
        Self {
            authority: CommandAuthority::memory(incarnation),
            hot: Arc::new(MemoryDetails(Arc::clone(&state))),
            cold: None,
            state,
            maximum_events: usize::MAX,
            maximum_outcomes: usize::MAX,
        }
    }

    pub fn with_capacity(maximum_events: usize) -> Self {
        Self {
            maximum_events,
            ..Self::new()
        }
    }

    /// Raw trusted storage access, not an authorized command endpoint.
    pub fn detail_reader(&self) -> Arc<dyn OutcomeDetailReader> {
        Arc::new(MemoryDetails(Arc::clone(&self.state)))
    }

    /// Substitutes readers only; the coordinator and lifecycle authority remain shared.
    #[must_use]
    pub fn with_detail_readers(
        mut self,
        hot: Arc<dyn OutcomeDetailReader>,
        cold: Option<Arc<dyn OutcomeDetailReader>>,
    ) -> Self {
        self.hot = hot;
        self.cold = cold;
        self
    }

    pub async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        <Self as EventHistory>::load(self, stream_id).await
    }

    #[must_use]
    pub const fn with_outcome_capacity(mut self, maximum_outcomes: usize) -> Self {
        self.maximum_outcomes = maximum_outcomes;
        self
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
impl StreamDirectory for InMemoryEventStore {
    async fn list_streams(
        &self,
        aggregate_type: &AggregateType,
    ) -> Result<Vec<StreamSummary>, EventStoreError> {
        let mut streams = {
            let state = self.state.lock().await;
            state
                .streams
                .iter()
                .filter(|(stream_id, _)| stream_id.aggregate_type() == aggregate_type)
                .filter_map(|(stream_id, events)| {
                    events
                        .last()
                        .map(|event| StreamSummary::new(stream_id.clone(), event.stream_version()))
                })
                .collect::<Vec<_>>()
        };
        streams.sort_by(|left, right| {
            left.stream_id()
                .aggregate_id()
                .cmp(right.stream_id().aggregate_id())
        });
        Ok(streams)
    }
}

#[async_trait]
#[allow(clippy::too_many_lines)]
impl EventStore for InMemoryEventStore {
    async fn load_completed_outcome_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<crate::InlineCompletedOutcome>, EventStoreError> {
        match self
            .lookup_outcome(&OutcomeKey::new(
                self.authority.clone(),
                context.clone(),
                operation.clone(),
            ))
            .await?
        {
            OutcomeLookup::Missing => Ok(None),
            OutcomeLookup::Completed(outcome) => {
                Ok(Some(outcome.to_inline().map_err(|e| recovery_error(&e))?))
            }
        }
    }

    async fn commit_outcome(
        &self,
        transaction: EventTransaction,
    ) -> Result<crate::CompletedAppendOutcome, EventStoreError> {
        let committed = self
            .begin_command()
            .await?
            .commit_outcome(transaction)
            .await?;
        let inline = committed
            .outcome()
            .to_inline()
            .map_err(|e| recovery_error(&e))?;
        Ok(if committed.is_exact_replay() {
            crate::CompletedAppendOutcome::ExactReplay(inline)
        } else {
            crate::CompletedAppendOutcome::Appended(inline)
        })
    }
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
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        let state = self.state.lock().await;
        Ok(state
            .transactions
            .get(&(None, operation_id.clone()))
            .map(StoredTransaction::acceptance)
            .transpose()?)
    }

    async fn load_transaction_receipt_in_context(
        &self,
        bounded_context: &BoundedContextName,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        let state = self.state.lock().await;
        Ok(state
            .transactions
            .get(&(Some(bounded_context.clone()), operation_id.clone()))
            .map(StoredTransaction::acceptance)
            .transpose()?)
    }

    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        if transaction.completion().is_some() {
            return Err(invalid("use commit_outcome for completed decisions"));
        }
        let (stored, replay) = self.append_transaction_inner(transaction).await?;
        let receipt = stored.acceptance()?;
        Ok(if replay {
            TransactionAppendOutcome::ExactReplay(receipt)
        } else {
            TransactionAppendOutcome::Appended(receipt)
        })
    }
}

impl InMemoryEventStore {
    #[allow(clippy::too_many_lines)]
    async fn append_transaction_inner(
        &self,
        transaction: EventTransaction,
    ) -> Result<(StoredTransaction, bool), EventStoreError> {
        let total_events = validate_transaction_item_limit(&transaction)?;
        let proposal = ProposalDigest::of(&transaction)?;
        let mut state = self.state.lock().await;
        let transaction_key = (
            transaction.bounded_context().cloned(),
            transaction.operation_id().clone(),
        );
        if let Some(previous) = state.transactions.get(&transaction_key) {
            if previous.proposal() == proposal {
                return Ok((previous.clone(), true));
            }
            return Err(identity_conflict(
                "transaction identity was reused with different content",
            ));
        }
        if state.transactions.len() >= self.maximum_outcomes {
            return Err(EventStoreError::new(
                EventStoreErrorKind::CapacityExhausted,
                "in-memory completed outcome capacity exhausted",
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
        if let Some(bounded_context) = transaction.bounded_context() {
            receipt = receipt.with_bounded_context(bounded_context.clone());
        }
        if let Some(correlation_id) = transaction.correlation_id() {
            receipt = receipt.with_correlation_id(correlation_id.clone());
        }
        if let Some(causation_id) = transaction.causation_id() {
            receipt = receipt.with_causation_id(causation_id.clone());
        }
        if let Some(scope) = transaction.security_scope() {
            receipt = receipt.with_security_scope(scope.clone());
        }
        // Prepare before mutating any journal state. No full proposal or inline
        // completion survives in permanent storage, even before expiry.
        let (stored, detail) = if transaction.bounded_context().is_some() {
            let (marker, bytes) =
                OutcomeMarker::prepare(self.authority.clone(), &transaction, receipt)?;
            let detail = (
                marker.detail_reference().clone(),
                DetailState::Active(bytes),
            );
            (StoredTransaction::Scoped(Box::new(marker)), Some(detail))
        } else {
            (
                StoredTransaction::Unscoped {
                    proposal,
                    receipt: Box::new(receipt),
                },
                None,
            )
        };

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
        state.transactions.insert(transaction_key, stored.clone());
        if let Some((reference, detail)) = detail {
            state.details.insert(reference, detail);
        }
        drop(state);
        Ok((stored, false))
    }
}

struct MemoryDetails(Arc<Mutex<State>>);

#[async_trait]
impl OutcomeDetailReader for MemoryDetails {
    async fn get(&self, reference: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        Ok(match self.0.lock().await.details.get(reference) {
            Some(DetailState::Active(bytes)) => DetailRead::Found(bytes.clone()),
            Some(DetailState::Expired) | None => DetailRead::AbsentHere,
        })
    }
}

impl InMemoryEventStore {
    fn validate_authority(&self, key: &OutcomeKey) -> Result<(), EventStoreError> {
        if key.authority() != &self.authority {
            return Err(EventStoreError::new(
                EventStoreErrorKind::ConfigurationMismatch,
                "outcome key belongs to another authority",
            ));
        }
        Ok(())
    }

    async fn resolve_marker(
        &self,
        marker: OutcomeMarker,
    ) -> Result<CompletedOutcome, EventStoreError> {
        // Every invocation consults the coordinator, even on reader-only clones.
        // An admitted read may finish across a concurrent expiry; no permit is cached.
        let expired = match self
            .state
            .lock()
            .await
            .details
            .get(marker.detail_reference())
        {
            Some(DetailState::Expired) => true,
            Some(DetailState::Active(_)) => false,
            None => {
                return Err(EventStoreError::new(
                    EventStoreErrorKind::CorruptHistory,
                    "completed outcome lifecycle authority is missing",
                ));
            }
        };
        let details = if expired {
            OutcomeDetails::Expired
        } else {
            crate::outcome::resolve_details(
                marker.detail_reference(),
                self.hot.as_ref(),
                self.cold.as_deref(),
            )
            .await?
        };
        Ok(CompletedOutcome::new(marker, details))
    }
}

#[async_trait]
impl OutcomeReader for InMemoryEventStore {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.validate_authority(key)?;
        let stored = self
            .state
            .lock()
            .await
            .transactions
            .get(&(
                Some(key.bounded_context().clone()),
                key.operation_id().clone(),
            ))
            .cloned();
        match stored {
            None => Ok(OutcomeLookup::Missing),
            Some(StoredTransaction::Scoped(marker)) => Ok(OutcomeLookup::Completed(
                self.resolve_marker(*marker).await?,
            )),
            Some(StoredTransaction::Unscoped { .. }) => Err(EventStoreError::new(
                EventStoreErrorKind::CorruptHistory,
                "scoped outcome has no marker",
            )),
        }
    }
}

struct MemoryCommandSession<'a>(&'a InMemoryEventStore);

#[async_trait]
impl EventHistory for MemoryCommandSession<'_> {
    async fn load(&self, stream: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.load(stream).await
    }
}

#[async_trait]
impl CommandCommitSession for MemoryCommandSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        crate::validate_completed_transaction(&proposal)?;
        let (stored, replay) = self.0.append_transaction_inner(proposal).await?;
        let StoredTransaction::Scoped(marker) = stored else {
            return Err(invalid("completed command requires scoped marker"));
        };
        let outcome = self.0.resolve_marker(*marker).await?;
        Ok(if replay {
            CompletedCommit::ExactReplay(outcome)
        } else {
            CompletedCommit::Appended(outcome)
        })
    }
}

#[async_trait]
impl CommandBackend for InMemoryEventStore {
    fn authority(&self) -> &CommandAuthority {
        &self.authority
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        Ok(Box::new(MemoryCommandSession(self)))
    }
}

#[async_trait]
impl OutcomeLifecycle for InMemoryEventStore {
    async fn expire_details(
        &self,
        key: &OutcomeKey,
        expected: &OutcomeRevision,
    ) -> Result<OutcomeRevision, OutcomeLifecycleError> {
        self.validate_authority(key)?;
        let mut state = self.state.lock().await;
        let Some(StoredTransaction::Scoped(marker)) = state.transactions.get(&(
            Some(key.bounded_context().clone()),
            key.operation_id().clone(),
        )) else {
            return Err(OutcomeLifecycleError::Missing);
        };
        if marker.revision() != expected {
            return Err(OutcomeLifecycleError::RevisionMismatch);
        }
        let reference = marker.detail_reference().clone();
        let detail = state.details.get_mut(&reference).ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::CorruptHistory,
                "cannot expire missing lifecycle evidence",
            )
        })?;
        // Atomic withdrawal and reclamation. Retain an occupied, payload-free slot.
        // No API can prepare or reinsert bytes for this reference afterwards.
        *detail = DetailState::Expired;
        drop(state);
        Ok(expected.clone())
    }
}

fn recovery_error(error: &crate::OutcomeRecoveryError) -> EventStoreError {
    EventStoreError::new(
        match error {
            crate::OutcomeRecoveryError::Expired => EventStoreErrorKind::CompletedDetailsExpired,
            crate::OutcomeRecoveryError::Archived { .. }
            | crate::OutcomeRecoveryError::Unavailable { .. } => {
                EventStoreErrorKind::CompletedDetailsUnavailable
            }
        },
        error.to_string(),
    )
}

fn validate_transaction(
    state: &State,
    transaction: &EventTransaction,
) -> Result<(), EventStoreError> {
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::{
        CommandCompletion, CommandExecutionMetadata, ContentFingerprint, EncodedRejection,
    };

    fn proposal() -> EventTransaction {
        let metadata = CommandExecutionMetadata::new(
            OperationId::new("secret-operation").unwrap(),
            ContentFingerprint::digest("command"),
        )
        .with_default_outcome_evidence::<()>();
        EventTransaction::new(
            metadata.operation_id().clone(),
            metadata.operation_fingerprint(),
            Vec::new(),
        )
        .with_bounded_context(BoundedContextName::new("life").unwrap())
        .with_completion(CommandCompletion::new(
            metadata.outcome_evidence().cloned(),
            Some(EncodedRejection {
                codec_id: "raw".to_owned(),
                codec_version: 1,
                bytes: b"sensitive-rejection-body".repeat(1000),
            }),
        ))
    }

    struct DetachedReader {
        bytes: Vec<u8>,
        calls: std::sync::atomic::AtomicUsize,
    }
    #[async_trait]
    impl OutcomeDetailReader for DetachedReader {
        async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(DetailRead::Found(self.bytes.clone()))
        }
    }

    #[tokio::test]
    async fn expiry_reclaims_the_only_owned_completion_body_and_retains_no_proposal_copy() {
        let store = InMemoryEventStore::new();
        let proposal = proposal();
        let commit = store
            .begin_command()
            .await
            .unwrap()
            .commit_outcome(proposal.clone())
            .await
            .unwrap();
        let marker = commit.outcome().marker();
        let reference = marker.detail_reference();
        // Compact detail encoding grows by raw bytes, not a JSON numeric byte array.
        let body_len = proposal
            .completion()
            .unwrap()
            .rejection
            .as_ref()
            .unwrap()
            .bytes
            .len();
        assert!(reference.length() < body_len.saturating_add(1024));
        store
            .expire_details(reference.key(), marker.revision())
            .await
            .unwrap();
        let state = store.state.lock().await;
        assert_eq!(state.transactions.len(), 1);
        assert_eq!(state.details.len(), 1);
        assert!(matches!(
            state.details.get(reference),
            Some(DetailState::Expired)
        ));
        // The permanent enum contains only marker/integrity or legacy acceptance;
        // there is no EventTransaction field in either branch.
        for stored in state.transactions.values() {
            match stored {
                StoredTransaction::Scoped(marker) => {
                    assert!(marker.integrity_receipt().completion().is_none());
                    assert!(marker.integrity_receipt().events().is_empty());
                    assert!(!format!("{marker:?}").contains("sensitive-rejection-body"));
                }
                StoredTransaction::Unscoped { receipt, .. } => {
                    assert!(receipt.completion().is_none());
                }
            }
        }
        drop(state);
        // The legacy full-result wrapper also fails closed; it cannot invent acceptance.
        assert_eq!(
            store
                .load_completed_outcome_in_context(
                    reference.key().bounded_context(),
                    reference.key().operation_id()
                )
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::CompletedDetailsExpired
        );
        assert_eq!(
            store
                .load_transaction_receipt_in_context(
                    reference.key().bounded_context(),
                    reference.key().operation_id()
                )
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::IdentityConflict
        );
        assert_eq!(
            store.commit_outcome(proposal).await.unwrap_err().kind(),
            EventStoreErrorKind::CompletedDetailsExpired
        );
        assert!(matches!(
            store.detail_reader().get(reference).await.unwrap(),
            DetailRead::AbsentHere
        ));
    }

    #[tokio::test]
    async fn missing_lifecycle_blocks_detached_hot_and_cold_even_with_a_valid_copy() {
        let store = InMemoryEventStore::new();
        let commit = store
            .begin_command()
            .await
            .unwrap()
            .commit_outcome(proposal())
            .await
            .unwrap();
        let marker = commit.outcome().marker();
        let reference = marker.detail_reference();
        let DetailRead::Found(bytes) = store.detail_reader().get(reference).await.unwrap() else {
            panic!("missing detail")
        };
        let reader = Arc::new(DetachedReader {
            bytes,
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let resolver = store
            .clone()
            .with_detail_readers(reader.clone(), Some(reader.clone()));
        // Fault injection, not an exported maintenance operation.
        store.state.lock().await.details.remove(reference);
        assert_eq!(
            resolver
                .lookup_outcome(reference.key())
                .await
                .unwrap_err()
                .kind(),
            EventStoreErrorKind::CorruptHistory
        );
        assert!(
            store
                .expire_details(reference.key(), marker.revision())
                .await
                .is_err()
        );
        assert_eq!(reader.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
