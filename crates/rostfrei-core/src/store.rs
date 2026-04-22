use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_messaging_core::{CausationId, CorrelationId};
use thiserror::Error;

use crate::{
    ContentFingerprint, EventBatch, ExpectedVersion, OperationId, RecordedEvent, StreamId,
    StreamVersion,
};

/// Maximum number of durable items in one atomic event transaction.
pub const MAX_TRANSACTION_ITEMS: usize = 1_000;

const TRANSACTION_RECEIPT_ITEMS: usize = 1;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionParticipant {
    stream_id: StreamId,
    expected_version: ExpectedVersion,
    batch: Option<EventBatch>,
}

impl TransactionParticipant {
    pub const fn new(
        stream_id: StreamId,
        expected_version: ExpectedVersion,
        batch: Option<EventBatch>,
    ) -> Self {
        Self {
            stream_id,
            expected_version,
            batch,
        }
    }

    pub const fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub const fn expected_version(&self) -> ExpectedVersion {
        self.expected_version
    }

    pub const fn batch(&self) -> Option<&EventBatch> {
        self.batch.as_ref()
    }

    pub fn into_parts(self) -> (StreamId, ExpectedVersion, Option<EventBatch>) {
        (self.stream_id, self.expected_version, self.batch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventTransaction {
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    participants: Vec<TransactionParticipant>,
}

impl EventTransaction {
    pub const fn new(
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
        participants: Vec<TransactionParticipant>,
    ) -> Self {
        Self {
            operation_id,
            operation_fingerprint,
            correlation_id: None,
            causation_id: None,
            participants,
        }
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

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn operation_fingerprint(&self) -> ContentFingerprint {
        self.operation_fingerprint
    }

    pub const fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub fn participants(&self) -> &[TransactionParticipant] {
        &self.participants
    }

    pub fn primary_stream_id(&self) -> Option<&StreamId> {
        self.participants
            .first()
            .map(TransactionParticipant::stream_id)
    }

    pub fn into_participants(self) -> Vec<TransactionParticipant> {
        self.participants
    }
}

/// Validates the transaction's primary participant and durable item limit.
///
/// Returns the number of domain events in the transaction.
pub fn validate_transaction_item_limit(
    transaction: &EventTransaction,
) -> Result<usize, EventStoreError> {
    let primary = transaction.participants().first().ok_or_else(|| {
        EventStoreError::new(
            EventStoreErrorKind::InvalidRequest,
            "an event transaction must contain at least one participant",
        )
    })?;
    if primary.batch().is_none() {
        return Err(EventStoreError::new(
            EventStoreErrorKind::InvalidRequest,
            "an event transaction's primary participant must contain an event batch",
        ));
    }
    let domain_event_count = transaction
        .participants()
        .iter()
        .filter_map(TransactionParticipant::batch)
        .try_fold(0_usize, |total, batch| {
            total.checked_add(batch.events().len())
        })
        .ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "transaction event count overflowed",
            )
        })?;
    let read_guard_count = transaction
        .participants()
        .iter()
        .filter(|participant| participant.batch().is_none())
        .count();
    let item_count = domain_event_count
        .checked_add(read_guard_count)
        .and_then(|count| count.checked_add(TRANSACTION_RECEIPT_ITEMS))
        .ok_or_else(|| {
            EventStoreError::new(
                EventStoreErrorKind::InvalidRequest,
                "transaction item count overflowed",
            )
        })?;
    if item_count > MAX_TRANSACTION_ITEMS {
        return Err(EventStoreError::new(
            EventStoreErrorKind::InvalidRequest,
            format!("transaction exceeds the {MAX_TRANSACTION_ITEMS}-item limit"),
        ));
    }
    Ok(domain_event_count)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionStreamReceipt {
    stream_id: StreamId,
    base_version: StreamVersion,
    events: Vec<RecordedEvent>,
}

impl TransactionStreamReceipt {
    pub const fn new(
        stream_id: StreamId,
        base_version: StreamVersion,
        events: Vec<RecordedEvent>,
    ) -> Self {
        Self {
            stream_id,
            base_version,
            events,
        }
    }

    pub const fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    pub const fn base_version(&self) -> StreamVersion {
        self.base_version
    }

    pub fn events(&self) -> &[RecordedEvent] {
        &self.events
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransactionReceipt {
    operation_id: OperationId,
    operation_fingerprint: ContentFingerprint,
    correlation_id: Option<CorrelationId>,
    causation_id: Option<CausationId>,
    streams: Vec<TransactionStreamReceipt>,
}

impl TransactionReceipt {
    pub const fn new(
        operation_id: OperationId,
        operation_fingerprint: ContentFingerprint,
        streams: Vec<TransactionStreamReceipt>,
    ) -> Self {
        Self {
            operation_id,
            operation_fingerprint,
            correlation_id: None,
            causation_id: None,
            streams,
        }
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

    pub const fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub const fn operation_fingerprint(&self) -> ContentFingerprint {
        self.operation_fingerprint
    }

    pub const fn correlation_id(&self) -> Option<&CorrelationId> {
        self.correlation_id.as_ref()
    }

    pub const fn causation_id(&self) -> Option<&CausationId> {
        self.causation_id.as_ref()
    }

    pub fn streams(&self) -> &[TransactionStreamReceipt] {
        &self.streams
    }

    pub fn primary_stream_id(&self) -> Option<&StreamId> {
        self.streams
            .first()
            .map(TransactionStreamReceipt::stream_id)
    }

    pub fn events(&self) -> Vec<RecordedEvent> {
        self.streams
            .iter()
            .flat_map(|stream| stream.events.iter().cloned())
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransactionAppendOutcome {
    Appended(TransactionReceipt),
    ExactReplay(TransactionReceipt),
}

impl TransactionAppendOutcome {
    pub const fn receipt(&self) -> &TransactionReceipt {
        match self {
            Self::Appended(receipt) | Self::ExactReplay(receipt) => receipt,
        }
    }

    pub fn into_receipt(self) -> TransactionReceipt {
        match self {
            Self::Appended(receipt) | Self::ExactReplay(receipt) => receipt,
        }
    }

    pub const fn is_exact_replay(&self) -> bool {
        matches!(self, Self::ExactReplay(_))
    }
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
pub trait EventHistory: Send + Sync {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError>;
}

#[async_trait]
pub trait EventStore: EventHistory {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError>;

    async fn load_transaction_receipt(
        &self,
        _primary_stream_id: &StreamId,
        _operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        Ok(None)
    }

    async fn append_transaction(
        &self,
        _transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        Err(EventStoreError::new(
            EventStoreErrorKind::ConfigurationMismatch,
            "event store does not support event transactions",
        ))
    }
}

#[async_trait]
impl<History: EventHistory + ?Sized> EventHistory for Arc<History> {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.as_ref().load(stream_id).await
    }
}

#[async_trait]
impl<Store: EventStore + ?Sized> EventStore for Arc<Store> {
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

    async fn load_transaction_receipt(
        &self,
        primary_stream_id: &StreamId,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.as_ref()
            .load_transaction_receipt(primary_stream_id, operation_id)
            .await
    }

    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.as_ref().append_transaction(transaction).await
    }
}
