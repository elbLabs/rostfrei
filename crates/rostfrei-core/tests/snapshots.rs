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

async fn counter_history() -> TestResult<(InMemoryEventStore, StreamId)> {
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
            .append_fixture(
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
    Ok((store, stream))
}

#[tokio::test]
async fn custom_state_codec_decodes_once_and_compatibility_reads_report_full_io() -> TestResult {
    let (store, stream) = counter_history().await?;
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
    assert_eq!(report.snapshot_warning, None);
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

async fn snapshot_fixture() -> TestResult<(InMemoryEventStore, StreamId, StoredSnapshot)> {
    let (history, stream) = counter_history().await?;
    let checkpoint = history
        .load_replay(&stream, None, Some(StreamVersion::new(7)))
        .await?
        .checkpoint
        .ok_or("missing checkpoint")?;
    let snapshot = StoredSnapshot::new(
        "counter-v1".to_owned(),
        1,
        checkpoint,
        28_u64.to_le_bytes().to_vec(),
    )?;
    Ok((history, stream, snapshot))
}

async fn snapshot_codecs(
    history: &dyn EventHistory,
    stream: &StreamId,
    snapshot: &StoredSnapshot,
) -> TestResult<(Arc<AggregateCodecs>, Arc<RebuildDiagnostics>)> {
    let snapshots = Arc::new(InMemorySnapshotStore::default());
    let key = SnapshotKey::new(
        history.history_authority().ok_or("missing authority")?,
        stream,
        "counter-v1",
        1,
    );
    snapshots.save(&key, snapshot).await?;
    let diagnostics = Arc::new(RebuildDiagnostics::new(1));
    let mut codecs = AggregateCodecs::default()
        .with_snapshot_store(snapshots)
        .with_rebuild_diagnostics(diagnostics.clone());
    codecs.register_snapshot::<Counter, _>(
        "counter-v1",
        1,
        BinaryState(Arc::default()),
        SnapshotPolicy::Manual,
    )?;
    Ok((Arc::new(codecs), diagnostics))
}

fn assert_snapshot_warning(report: &RebuildReport, expected: &str) -> TestResult {
    let serialized = serde_json::to_value(report)?;
    let warning = serialized
        .get("snapshot_warning")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing nonfatal snapshot warning")?;
    assert!(warning.contains(expected), "unexpected warning: {warning}");
    assert_eq!(report.error, None);
    Ok(())
}

async fn assert_bad_digest_fallback(through: Option<StreamVersion>) -> TestResult {
    let (history, stream, mut snapshot) = snapshot_fixture().await?;
    snapshot.checkpoint.position = b"invalid cached prefix digest".to_vec();
    let snapshot = StoredSnapshot::new(
        snapshot.replay_revision,
        snapshot.schema_version,
        snapshot.checkpoint,
        snapshot.state,
    )?;
    snapshot.validate()?;
    let (codecs, diagnostics) = snapshot_codecs(&history, &stream, &snapshot).await?;
    let executor = CommandExecutor::with_codecs(history, codecs);
    let aggregate = match through {
        Some(version) => executor.rehydrate_at::<Counter>(&stream, version).await?,
        None => executor.rehydrate::<Counter>(&stream).await?,
    };
    let (expected_version, expected_state, expected_applied) = if through.is_some() {
        (8, 36, 8)
    } else {
        (10, 55, 10)
    };
    assert_eq!(aggregate.state().0, expected_state);
    let report = diagnostics.recent().pop().ok_or("missing diagnostic")?;
    assert_eq!(report.snapshot_status, SnapshotStatus::InvalidAnchor);
    assert_eq!(report.snapshot_version, 0);
    assert_eq!(report.target_version, through.map(StreamVersion::value));
    assert_eq!(report.head_version, expected_version);
    assert_eq!(report.events_applied, expected_applied);
    assert_eq!(report.events_read, 10);
    assert_snapshot_warning(&report, "checkpoint prefix does not match history")
}

#[tokio::test]
async fn bad_cache_digest_replays_full_history_with_warning() -> TestResult {
    assert_bad_digest_fallback(None).await
}

#[tokio::test]
async fn bad_cache_digest_fallback_preserves_the_replay_target() -> TestResult {
    assert_bad_digest_fallback(Some(StreamVersion::new(8))).await
}

struct CorruptedHistory {
    authority: String,
    events: Vec<RecordedEvent>,
    loads: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl EventHistory for CorruptedHistory {
    async fn load(&self, _: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        Ok(self.events.clone())
    }

    fn history_authority(&self) -> Option<&str> {
        Some(&self.authority)
    }
}

#[tokio::test]
async fn corrupt_authoritative_history_fails_without_cache_fallback() -> TestResult {
    let (history, stream, snapshot) = snapshot_fixture().await?;
    let (codecs, diagnostics) = snapshot_codecs(&history, &stream, &snapshot).await?;
    let mut events = history.load(&stream).await?;
    events.remove(3);
    let loads = Arc::new(AtomicUsize::new(0));
    let history = CorruptedHistory {
        authority: history
            .history_authority()
            .ok_or("missing authority")?
            .to_owned(),
        events,
        loads: loads.clone(),
    };
    let executor = CommandExecutor::with_codecs(history, codecs);
    assert!(matches!(
        executor.rehydrate::<Counter>(&stream).await,
        Err(SimulationError::Store(ref error))
            if error.kind() == EventStoreErrorKind::CorruptHistory
                && error.message().contains("non-contiguous stream versions")
    ));
    assert_eq!(loads.load(Ordering::Relaxed), 1);
    let report = diagnostics.recent().pop().ok_or("missing diagnostic")?;
    assert!(report.error.is_some());
    assert_eq!(report.snapshot_warning, None);
    assert_eq!(report.events_applied, 0);
    Ok(())
}

#[tokio::test]
async fn undecodable_cached_state_replays_history_with_warning() -> TestResult {
    let (history, stream, snapshot) = snapshot_fixture().await?;
    let snapshot = StoredSnapshot::new(
        snapshot.replay_revision,
        snapshot.schema_version,
        snapshot.checkpoint,
        vec![0],
    )?;
    let (codecs, diagnostics) = snapshot_codecs(&history, &stream, &snapshot).await?;
    let executor = CommandExecutor::with_codecs(history, codecs);
    assert_eq!(executor.rehydrate::<Counter>(&stream).await?.state().0, 55);
    let report = diagnostics.recent().pop().ok_or("missing diagnostic")?;
    assert_eq!(report.snapshot_status, SnapshotStatus::InvalidState);
    assert_eq!(report.snapshot_version, 0);
    assert_eq!(report.events_applied, 10);
    assert_snapshot_warning(&report, "expected eight bytes")
}

#[tokio::test]
async fn foreign_cached_checkpoint_replays_history_with_warning() -> TestResult {
    let (history, stream, mut snapshot) = snapshot_fixture().await?;
    snapshot.checkpoint.authority = "another-history".to_owned();
    let snapshot = StoredSnapshot::new(
        snapshot.replay_revision,
        snapshot.schema_version,
        snapshot.checkpoint,
        snapshot.state,
    )?;
    let (codecs, diagnostics) = snapshot_codecs(&history, &stream, &snapshot).await?;
    let executor = CommandExecutor::with_codecs(history, codecs);
    assert_eq!(executor.rehydrate::<Counter>(&stream).await?.state().0, 55);
    let report = diagnostics.recent().pop().ok_or("missing diagnostic")?;
    assert_eq!(report.snapshot_status, SnapshotStatus::InvalidCheckpoint);
    assert_eq!(report.snapshot_version, 0);
    assert_eq!(report.events_applied, 10);
    assert_snapshot_warning(&report, "checkpoint belongs to another history")
}

#[tokio::test]
async fn pending_snapshot_work_survives_authority_replacement() -> TestResult {
    let mut codecs =
        AggregateCodecs::default().with_snapshot_store(Arc::new(InMemorySnapshotStore::default()));
    codecs.register_snapshot::<Counter, _>(
        "counter-v1",
        1,
        BinaryState(Arc::default()),
        SnapshotPolicy::EveryEvents(std::num::NonZeroU64::new(1).ok_or("nonzero")?),
    )?;
    let codecs = Arc::new(codecs);
    let old = InMemoryEventStore::new();
    let executor = CommandExecutor::with_codecs(old.clone(), codecs.clone());
    for index in 0..127 {
        let stream = StreamId::new(
            AggregateType::new("counter")?,
            AggregateId::new(format!("queued-{index}"))?,
        );
        let operation = OperationId::new(format!("seed-{index}"))?;
        let commit = derive_commit_id(&stream, &operation);
        let event = <JsonEventCodec as EventCodec<Counter>>::encode(
            &JsonEventCodec,
            &Added(1),
            derive_event_id(&commit, 0),
        )?;
        old.append_fixture(
            &stream,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest(b"seed"),
                vec![event],
            )?,
        )
        .await?;
        executor.rehydrate::<Counter>(&stream).await?;
    }
    let (other, other_stream) = counter_history().await?;
    let other_executor = CommandExecutor::with_codecs(other, codecs.clone());
    other_executor.rehydrate::<Counter>(&other_stream).await?;
    assert_eq!(codecs.pending_snapshots().len(), 128);
    drop(executor);
    drop(old);

    let (replacement, stream) = counter_history().await?;
    let executor = CommandExecutor::with_codecs(replacement, codecs.clone());
    assert!(executor.run_snapshot_jobs(128).await.is_empty());
    assert_eq!(executor.rehydrate::<Counter>(&stream).await?.state().0, 55);
    let jobs = executor.run_snapshot_jobs(128).await;
    assert_eq!(
        jobs.len(),
        1,
        "retired-authority work must not permanently prevent snapshots in the replacement"
    );
    assert!(jobs.into_iter().all(|result| result.is_ok()));
    let other_jobs = other_executor.run_snapshot_jobs(128).await;
    assert_eq!(
        other_jobs.len(),
        1,
        "another live authority retains its work"
    );
    assert!(other_jobs.into_iter().all(|result| result.is_ok()));
    Ok(())
}

#[tokio::test]
async fn in_memory_snapshot_publication_refreshes_only_checkpoints() -> TestResult {
    let (history, stream, original) = snapshot_fixture().await?;
    let key = SnapshotKey::new(
        history.history_authority().ok_or("missing authority")?,
        &stream,
        "counter-v1",
        1,
    );
    let mut checkpoint = original.checkpoint.clone();
    checkpoint.position = b"invalid cached prefix digest".to_vec();
    let broken = StoredSnapshot::new(
        original.replay_revision.clone(),
        original.schema_version,
        checkpoint,
        original.state.clone(),
    )?;
    let snapshots = InMemorySnapshotStore::default();
    snapshots.save(&key, &broken).await?;
    snapshots.save(&key, &original).await?;
    assert_eq!(
        snapshots.load(&key, original.version()).await?,
        Some(original.clone())
    );
    let different_state = StoredSnapshot::new(
        original.replay_revision.clone(),
        original.schema_version,
        original.checkpoint.clone(),
        99_u64.to_le_bytes().to_vec(),
    )?;
    let conflict = snapshots.save(&key, &different_state).await;
    assert!(matches!(conflict, Err(error) if error.kind() == EventStoreErrorKind::Conflict));
    assert_eq!(
        snapshots.load(&key, original.version()).await?,
        Some(original)
    );
    Ok(())
}
