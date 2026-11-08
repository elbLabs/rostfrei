//! Contracts owned by the testing helpers. Executor behavior lives in core/runtime suites.
#![allow(clippy::unwrap_used)]
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, AppendOutcome, CommittedDomainEvent, ContentFingerprint,
    DomainEventDispatchOutcome, DomainEventHandler, DomainEventHandlerError,
    DomainEventHandlerErrorKind, EventBatch, EventCodec, EventCodecError, EventCodecErrorKind,
    EventHistory, EventStoreError, ExpectedVersion, FixtureEventStore, InMemoryEventStore,
    NewEvent, OperationId, RecordedEvent, StreamId, StreamVersion, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::CorrelationId;
use rostfrei_testing::{DomainEventHandlerHarness, event_store_contract, given};

#[tokio::test]
async fn in_memory_store_satisfies_reusable_contracts() {
    event_store_contract::run(InMemoryEventStore::new).await;
    event_store_contract::run_atomic_multi_stream_transactions(InMemoryEventStore::new).await;
}

struct FixtureOnlyStore(InMemoryEventStore);

#[async_trait]
impl EventHistory for FixtureOnlyStore {
    async fn load(&self, stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.0.load(stream_id).await
    }
}

#[async_trait]
impl FixtureEventStore for FixtureOnlyStore {
    async fn append_fixture(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.0
            .append_fixture(stream_id, expected_version, batch)
            .await
    }
}

#[test]
fn reusable_base_contract_accepts_fixture_only_stores() {
    // Instantiating the future checks the runner's bounds without rerunning memory conformance.
    drop(event_store_contract::run(|| {
        FixtureOnlyStore(InMemoryEventStore::new())
    }));
}

#[tokio::test]
async fn reusable_contract_reports_errors_and_legacy_wrapper_fails_closed() {
    let result = event_store_contract::try_run(|| InMemoryEventStore::with_capacity(0)).await;
    assert!(matches!(
        result,
        Err(event_store_contract::ContractTestError::Store {
            context: "NoStream should append to an absent stream",
            ..
        })
    ));
    let assertion = tokio::spawn(async {
        event_store_contract::run(|| InMemoryEventStore::with_capacity(0)).await;
    })
    .await;
    assert!(matches!(assertion, Err(error) if error.is_panic()));
}

struct Counter;
impl Aggregate for Counter {
    type State = u64;
    type Event = u8;
    const BOUNDED_CONTEXT: &'static str = "testing";
    const AGGREGATE_TYPE: &'static str = "counter";
    fn initial(_: &StreamId) -> u64 {
        0
    }
    fn apply(state: &mut u64, event: &u8) {
        *state = state.saturating_add(u64::from(*event));
    }
}
fn stream() -> StreamId {
    StreamId::new(
        AggregateType::new("counter").unwrap(),
        AggregateId::new("one").unwrap(),
    )
}

#[test]
fn given_when_then_exposes_live_state_and_replay_equivalence() {
    let id = stream();
    let mut history = vec![10];
    let then = given::<Counter, _>(&id, history.clone()).when(|aggregate| {
        aggregate.raise(5);
        assert_eq!(*aggregate.state(), 15);
        aggregate.raise(2);
    });
    assert_eq!(then.events(), &[5, 2]);
    assert_eq!(*then.state(), 17);
    let (live_state, events) = then.into_parts();
    history.extend(events);
    assert_eq!(given::<Counter, _>(&id, history).state(), &live_state);
}

struct ByteCodec;
impl EventCodec<Counter> for ByteCodec {
    fn encode(&self, event: &u8, id: rostfrei_core::EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(id, "added", 1, vec![*event])
            .map_err(|e| EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, e.to_string()))
    }
    fn decode(&self, event: &RecordedEvent) -> Result<u8, EventCodecError> {
        match event.payload() {
            [value] => Ok(*value),
            _ => Err(EventCodecError::new(
                EventCodecErrorKind::MalformedPayload,
                "expected one byte",
            )),
        }
    }
}
#[derive(Default)]
struct Handler(Mutex<Vec<(u8, RecordedEvent)>>);
#[async_trait]
impl DomainEventHandler<u8> for Handler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, u8>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0
            .lock()
            .unwrap()
            .push((*event.event(), event.recorded().clone()));
        Ok(())
    }
}

#[tokio::test]
async fn handler_harness_uses_the_supplied_codec_and_preserves_recorded_evidence() {
    let id = stream();
    let operation = OperationId::new("operation").unwrap();
    let commit = derive_commit_id(&id, &operation);
    let recorded = |payload| {
        RecordedEvent::new(
            id.clone(),
            StreamVersion::new(1),
            derive_event_id(&commit, 0),
            commit.clone(),
            operation.clone(),
            ContentFingerprint::digest("operation"),
            "added",
            1,
            payload,
        )
        .unwrap()
        .with_correlation_id(CorrelationId::new("flow").unwrap())
    };
    let handler = Arc::new(Handler::default());
    let mut harness = DomainEventHandlerHarness::new();
    harness
        .register_with_codec::<Counter, u8, _, _>("added", Arc::new(ByteCodec), handler.clone())
        .unwrap();
    let event = recorded(vec![7]);
    assert_eq!(
        harness.handle(&event).await.unwrap(),
        DomainEventDispatchOutcome::Handled
    );
    assert_eq!(*handler.0.lock().unwrap(), [(7, event)]);
    assert_eq!(
        harness.handle(&recorded(vec![])).await.unwrap_err().kind(),
        DomainEventHandlerErrorKind::InvalidCommittedEvent
    );
    assert_eq!(handler.0.lock().unwrap().len(), 1);
}
