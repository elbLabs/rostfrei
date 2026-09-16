use async_trait::async_trait;

use crate::{
    AppendOutcome, EventBatch, EventHistory, EventStore, EventStoreError, EventTransaction,
    ExpectedVersion, RecordedEvent, StreamId, TransactionAppendOutcome,
};

#[async_trait]
pub trait AppendSession: EventHistory {
    async fn append(
        self: Box<Self>,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError>;

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
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.store.load(stream_id).await
    }
}

#[async_trait]
impl<S: EventStore + ?Sized> AppendSession for ForwardingAppendSession<'_, S> {
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
