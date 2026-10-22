//! Custom non-Serde states and compatibility readers share the snapshot loader.
#![allow(clippy::panic_in_result_fn)]
use rostfrei_core::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Counter;
struct State(u64);
struct Added(u64);
impl Aggregate for Counter {
    type State = State;
    type Event = Added;
    const BOUNDED_CONTEXT: &'static str = "snapshots";
    const AGGREGATE_TYPE: &'static str = "counter";
    fn initial(_: &StreamId) -> State {
        State(0)
    }
    fn apply(state: &mut State, event: &Added) {
        state.0 = state.0.saturating_add(event.0);
    }
}
impl Event for Added {
    fn event_type(&self) -> &'static str {
        "added"
    }
    fn schema_version(&self) -> u32 {
        1
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        Ok(self.0.to_string().into_bytes())
    }
    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        serde_json::from_slice(event.payload())
            .map(Self)
            .map_err(|error| {
                EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
            })
    }
}

struct BinaryState(Arc<AtomicUsize>);
impl SnapshotCodec<Counter> for BinaryState {
    fn encode(&self, state: &State) -> Result<Vec<u8>, EventCodecError> {
        Ok(state.0.to_le_bytes().to_vec())
    }
    fn decode(&self, bytes: &[u8]) -> Result<State, EventCodecError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let bytes = bytes.try_into().map_err(|_| {
            EventCodecError::new(
                EventCodecErrorKind::MalformedPayload,
                "expected eight bytes",
            )
        })?;
        Ok(State(u64::from_le_bytes(bytes)))
    }
}

#[tokio::test]
async fn custom_state_codec_decodes_once_and_compatibility_reads_report_full_io() -> TestResult {
    let store = InMemoryEventStore::new();
    let stream = StreamId::new(AggregateType::new("counter")?, AggregateId::new("one")?);
    for number in 1..=10_u64 {
        let operation = OperationId::new(format!("add-{number}"))?;
        let commit = derive_commit_id(&stream, &operation);
        let event = <JsonEventCodec as EventCodec<Counter>>::encode(
            &JsonEventCodec,
            &Added(number),
            derive_event_id(&commit, 0),
        )?;
        let expected = if number == 1 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(number.saturating_sub(1)))
        };
        store
            .append(
                &stream,
                expected,
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(number.to_le_bytes()),
                    vec![event],
                )?,
            )
            .await?;
    }
    let decodes = Arc::new(AtomicUsize::new(0));
    let diagnostics = Arc::new(RebuildDiagnostics::new(2));
    let mut codecs = AggregateCodecs::default()
        .with_snapshot_store(Arc::new(InMemorySnapshotStore::default()))
        .with_rebuild_diagnostics(diagnostics.clone());
    codecs.register_snapshot::<Counter, _>(
        "counter-v1",
        1,
        BinaryState(decodes.clone()),
        SnapshotPolicy::Manual,
    )?;
    let executor = CommandExecutor::with_codecs(store, Arc::new(codecs));
    executor
        .create_snapshot::<Counter>(&stream, StreamVersion::new(7))
        .await?;
    decodes.store(0, Ordering::Relaxed);
    assert_eq!(executor.rehydrate::<Counter>(&stream).await?.state().0, 55);
    assert_eq!(decodes.load(Ordering::Relaxed), 1);
    let report = diagnostics.recent().pop().ok_or("missing diagnostic")?;
    assert_eq!(
        (
            report.snapshot_version,
            report.events_read,
            report.events_applied
        ),
        (7, 10, 3)
    );
    assert!(
        executor
            .verify_snapshot::<Counter>(&stream, StreamVersion::new(7))
            .await?
    );
    assert_eq!(
        executor
            .rehydrate_with_codec::<Counter, _>(&stream, &JsonEventCodec)
            .await?
            .state()
            .0,
        55
    );
    let report = diagnostics
        .recent()
        .pop()
        .ok_or("missing override diagnostic")?;
    assert_eq!(report.snapshot_status, SnapshotStatus::Disabled);
    assert_eq!(report.events_applied, 10);
    assert_eq!(diagnostics.observed(), 4);
    assert_eq!(diagnostics.recent().len(), 2);
    assert_eq!(diagnostics.slowest(1).len(), 1);
    Ok(())
}
