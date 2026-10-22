use async_trait::async_trait;

use crate::{
    AppendOutcome, EventBatch, EventHistory, EventStore, EventStoreError, EventTransaction,
    ExpectedVersion, RecordedEvent, StreamId, TransactionAppendOutcome,
};

#[async_trait]
pub trait AppendSession: EventHistory {
    /// Consumes one guarded attempt, accepted or rejected, in the same atomic namespace.
    async fn commit_outcome(
        self: Box<Self>,
        transaction: EventTransaction,
    ) -> Result<crate::CompletedAppendOutcome, EventStoreError>;
    async fn append(
        self: Box<Self>,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError>;

    /// Consumes this attempt to atomically persist its acceptance and any events/read guards.
    /// Event-free and participant-free transactions follow [`EventStore::append_transaction`].
    async fn append_transaction(
        self: Box<Self>,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError>;
}

pub struct ForwardingAppendSession<'a, S: ?Sized> {
    pub store: &'a S,
}

#[async_trait]
impl<S: EventStore + ?Sized> EventHistory for ForwardingAppendSession<'_, S> {
    fn history_authority(&self) -> Option<&str> {
        self.store.history_authority()
    }

    async fn load_replay(
        &self,
        stream_id: &StreamId,
        after: Option<&crate::HistoryCheckpoint>,
        through: Option<crate::StreamVersion>,
    ) -> Result<crate::ReplayHistory, EventStoreError> {
        self.store.load_replay(stream_id, after, through).await
    }

    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.store.load(stream_id).await
    }
}

#[async_trait]
impl<S: EventStore + ?Sized> AppendSession for ForwardingAppendSession<'_, S> {
    async fn commit_outcome(
        self: Box<Self>,
        transaction: EventTransaction,
    ) -> Result<crate::CompletedAppendOutcome, EventStoreError> {
        self.store.commit_outcome(transaction).await
    }
    async fn append(
        self: Box<Self>,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.store.append(stream_id, expected_version, batch).await
    }

    async fn append_transaction(
        self: Box<Self>,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.store.append_transaction(transaction).await
    }
}
