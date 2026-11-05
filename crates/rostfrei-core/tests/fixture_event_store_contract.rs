//! Explicit fixture writes retain aggregate-local identity without recording command decisions.
#![allow(clippy::panic_in_result_fn)]

use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_core::{
    AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EventBatch, EventHistory,
    EventStore, EventStoreError, ExpectedVersion, FixtureEventStore, InMemoryEventStore, NewEvent,
    OperationId, RecordedEvent, StreamId, derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::BoundedContextName;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn stream(id: &str) -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new("fixture-account")?,
        AggregateId::new(id)?,
    ))
}

fn batch(stream: &StreamId, operation: &str, payloads: &[&[u8]]) -> TestResult<EventBatch> {
    let operation = OperationId::new(operation)?;
    let commit = derive_commit_id(stream, &operation);
    let events = payloads
        .iter()
        .enumerate()
        .map(|(ordinal, payload)| {
            Ok(NewEvent::new(
                derive_event_id(&commit, u32::try_from(ordinal)?),
                "account-seeded",
                1,
                payload.to_vec(),
            )?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    Ok(EventBatch::new(
        commit,
        operation.clone(),
        ContentFingerprint::digest(operation.as_str()),
        events,
    )?)
}

// This adapter deliberately implements only history and fixture persistence. An
// EventStore supertrait would unnecessarily require command decisions and sessions.
#[derive(Default)]
struct FixtureOnlyStore(InMemoryEventStore);

#[async_trait]
impl EventHistory for FixtureOnlyStore {
    async fn load(&self, stream: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.load(stream).await
    }
}

#[async_trait]
impl FixtureEventStore for FixtureOnlyStore {
    async fn append_fixture(
        &self,
        stream: &StreamId,
        expected: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.0.append_fixture(stream, expected, batch).await
    }
}

#[tokio::test]
async fn fixture_capability_is_object_safe_and_arc_forwarded_without_command_storage() -> TestResult
{
    let store: Arc<dyn FixtureEventStore> = Arc::new(FixtureOnlyStore::default());
    let stream = stream("capability-only")?;
    // UFCS requires the Arc implementation rather than relying on method autoderef.
    let appended = FixtureEventStore::append_fixture(
        &store,
        &stream,
        ExpectedVersion::NoStream,
        batch(&stream, "fixture-capability", &[b"seed"])?,
    )
    .await?;

    assert!(!appended.is_exact_replay());
    assert_eq!(appended.events().len(), 1);
    assert_eq!(
        EventHistory::load(&store, &stream).await?,
        appended.events()
    );
    Ok(())
}

#[tokio::test]
async fn fixture_events_do_not_create_command_acceptance_or_completed_outcomes() -> TestResult {
    let store = InMemoryEventStore::new();
    let stream = stream("no-command-decision")?;
    let context = BoundedContextName::new("fixture-context")?;
    let fixture = batch(&stream, "fixture-is-not-a-command", &[b"seed"])?;
    let operation = fixture.operation_id().clone();

    store
        .append_fixture(&stream, ExpectedVersion::NoStream, fixture)
        .await?;

    assert_eq!(store.load(&stream).await?.len(), 1);
    assert!(store.load_transaction_receipt(&operation).await?.is_none());
    assert!(
        store
            .load_transaction_receipt_in_context(&context, &operation)
            .await?
            .is_none()
    );
    assert!(
        store
            .load_completed_outcome_in_context(&context, &operation)
            .await?
            .is_none()
    );
    Ok(())
}
