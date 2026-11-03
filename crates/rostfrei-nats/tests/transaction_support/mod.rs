//! Single-participant command transactions for native integration scenarios.
use async_trait::async_trait;
use rostfrei_core::{
    AppendOutcome, AppendSession, EventBatch, EventStore, EventStoreError, EventTransaction,
    ExpectedVersion, StreamId, TransactionAppendOutcome, TransactionParticipant,
};

pub fn transaction(
    stream: &StreamId,
    expected: ExpectedVersion,
    batch: EventBatch,
) -> EventTransaction {
    let correlation = batch.correlation_id().cloned();
    let causation = batch.causation_id().cloned();
    let actor = batch.actor().cloned();
    let mut transaction = EventTransaction::new(
        batch.operation_id().clone(),
        batch.operation_fingerprint(),
        vec![TransactionParticipant::new(
            stream.clone(),
            expected,
            Some(batch),
        )],
    );
    if let Some(correlation) = correlation {
        transaction = transaction.with_correlation_id(correlation);
    }
    if let Some(causation) = causation {
        transaction = transaction.with_causation_id(causation);
    }
    if let Some(actor) = actor {
        transaction = transaction.with_actor(actor);
    }
    transaction
}

fn events(outcome: TransactionAppendOutcome) -> AppendOutcome {
    match outcome {
        TransactionAppendOutcome::Appended(receipt) => AppendOutcome::Appended(receipt.events()),
        TransactionAppendOutcome::ExactReplay(receipt) => {
            AppendOutcome::ExactReplay(receipt.events())
        }
    }
}

#[async_trait]
pub trait AppendOneTransaction {
    async fn append_one_transaction(
        self,
        stream: &StreamId,
        expected: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError>;
}

#[async_trait]
impl<S: EventStore> AppendOneTransaction for &S {
    async fn append_one_transaction(
        self,
        stream: &StreamId,
        expected: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.append_transaction(transaction(stream, expected, batch))
            .await
            .map(events)
    }
}

#[async_trait]
impl AppendOneTransaction for Box<dyn AppendSession + '_> {
    async fn append_one_transaction(
        self,
        stream: &StreamId,
        expected: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.append_transaction(transaction(stream, expected, batch))
            .await
            .map(events)
    }
}
