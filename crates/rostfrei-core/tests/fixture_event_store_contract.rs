//! Explicit fixture writes retain aggregate-local identity without recording command decisions.
#![allow(clippy::panic_in_result_fn)]

use std::sync::Arc;

use async_trait::async_trait;
use rostfrei_core::{
    ActorRef, AggregateId, AggregateType, AppendOutcome, ContentFingerprint, EnvelopeError,
    EventBatch, EventHistory, EventStore, EventStoreError, EventStoreErrorKind, ExpectedVersion,
    FixtureEventStore, InMemoryEventStore, NewEvent, OperationId, RecordedEvent, StreamId,
    StreamVersion, derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::{BoundedContextName, CausationId, CorrelationId};

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
async fn fixture_operation_ids_are_aggregate_scoped() -> TestResult {
    let store = InMemoryEventStore::new();
    let first = stream("one")?;
    let second = stream("two")?;
    let first_batch = batch(&first, "shared-fixture-operation", &[b"first account"])?;
    let second_batch = batch(&second, "shared-fixture-operation", &[b"second account"])?;

    let first_append = store
        .append_fixture(&first, ExpectedVersion::NoStream, first_batch.clone())
        .await?;
    let second_append = store
        .append_fixture(&second, ExpectedVersion::NoStream, second_batch.clone())
        .await?;

    assert!(!first_append.is_exact_replay());
    assert!(!second_append.is_exact_replay());
    assert_ne!(first_batch.commit_id(), second_batch.commit_id());
    for (stream, batch, appended) in [
        (&first, first_batch, first_append),
        (&second, second_batch, second_append),
    ] {
        let replay = store
            .append_fixture(stream, ExpectedVersion::NoStream, batch)
            .await?;
        assert!(replay.is_exact_replay());
        assert_eq!(replay.events(), appended.events());
        assert_eq!(store.load(stream).await?, appended.events());
    }
    Ok(())
}

#[tokio::test]
async fn fixture_retry_preserves_original_events_and_rejects_changed_content_or_provenance()
-> TestResult {
    let store = InMemoryEventStore::new();
    let stream = stream("retry")?;
    let correlation = CorrelationId::new("fixture-correlation")?;
    let causation = CausationId::new("fixture-parent")?;
    let actor = ActorRef::new("test-authors", "alice")?;
    let original = batch(&stream, "fixture-retry", &[b"first", b"second"])?
        .with_correlation_id(correlation.clone())
        .with_causation_id(causation.clone())
        .with_actor(actor.clone());
    let appended = store
        .append_fixture(&stream, ExpectedVersion::NoStream, original.clone())
        .await?;
    store
        .append_fixture(
            &stream,
            ExpectedVersion::Exact(StreamVersion::new(2)),
            batch(&stream, "later-fixture", &[b"later"])?,
        )
        .await?;
    let history = store.load(&stream).await?;

    let replay = store
        .append_fixture(&stream, ExpectedVersion::NoStream, original.clone())
        .await?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.events(), appended.events());
    assert_eq!(replay.events().len(), 2);
    for event in replay.events() {
        assert_eq!(event.correlation_id(), Some(&correlation));
        assert_eq!(event.causation_id(), Some(&causation));
        assert_eq!(event.actor(), Some(&actor));
    }

    let changed_payload = batch(&stream, "fixture-retry", &[b"changed", b"second"])?
        .with_correlation_id(correlation.clone())
        .with_causation_id(causation.clone())
        .with_actor(actor.clone());
    let changed_fingerprint = EventBatch::new(
        original.commit_id().clone(),
        original.operation_id().clone(),
        ContentFingerprint::digest("different fixture document"),
        original.events().to_vec(),
    )?
    .with_correlation_id(correlation)
    .with_causation_id(causation)
    .with_actor(actor);
    for changed in [
        changed_payload,
        changed_fingerprint,
        original
            .clone()
            .with_correlation_id(CorrelationId::new("different-correlation")?),
        original
            .clone()
            .with_causation_id(CausationId::new("different-parent")?),
        original.with_actor(ActorRef::new("test-authors", "bob")?),
    ] {
        let error = store
            .append_fixture(&stream, ExpectedVersion::NoStream, changed)
            .await
            .err()
            .ok_or("changed fixture evidence must not replay the original write")?;
        assert_eq!(error.kind(), EventStoreErrorKind::IdentityConflict);
        assert_eq!(store.load(&stream).await?, history);
    }
    Ok(())
}

#[tokio::test]
async fn a_fixture_batch_retains_the_full_hundred_domain_event_allowance() -> TestResult {
    let store = InMemoryEventStore::new();
    let stream = stream("maximum-batch")?;
    let payloads: Vec<&[u8]> = vec![b"seed"; 100];
    let maximum = batch(&stream, "fixture-maximum", &payloads)?;
    let appended = store
        .append_fixture(&stream, ExpectedVersion::NoStream, maximum.clone())
        .await?;

    assert_eq!(appended.events().len(), 100);
    for (ordinal, event) in appended.events().iter().enumerate() {
        assert_eq!(event.stream_version().value(), u64::try_from(ordinal)? + 1);
        assert_eq!(event.commit_event_ordinal(), u32::try_from(ordinal)?);
        assert_eq!(event.commit_event_count(), 100);
    }
    assert_eq!(store.load(&stream).await?, appended.events());
    let replay = store
        .append_fixture(&stream, ExpectedVersion::NoStream, maximum.clone())
        .await?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.events(), appended.events());

    let mut oversized = maximum.events().to_vec();
    oversized.push(NewEvent::new(
        derive_event_id(maximum.commit_id(), 100),
        "account-seeded",
        1,
        b"seed".to_vec(),
    )?);
    assert!(matches!(
        EventBatch::new(
            maximum.commit_id().clone(),
            maximum.operation_id().clone(),
            maximum.operation_fingerprint(),
            oversized,
        ),
        Err(EnvelopeError::BatchTooLarge {
            actual: 101,
            maximum: 100,
        })
    ));
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
