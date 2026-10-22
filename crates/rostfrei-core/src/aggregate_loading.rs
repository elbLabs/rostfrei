use crate::{
    Aggregate, AggregateCodecs, AggregateInstance, CommandExecutionError, Event, EventCodec,
    EventHistory, RebuildReport, SnapshotCodec, SnapshotKey, SnapshotPolicy, SnapshotStatus,
    StoredSnapshot, StreamId, StreamVersion,
};
use async_trait::async_trait;
use std::{any::Any, marker::PhantomData, sync::Arc, time::Instant};

type DecodeState =
    dyn Fn(&[u8]) -> Result<Box<dyn Any + Send + Sync>, crate::EventCodecError> + Send + Sync;

#[derive(Clone)]
pub struct SnapshotEntry {
    pub revision: String,
    pub schema: u32,
    pub policy: SnapshotPolicy,
    pub codec: Arc<dyn Any + Send + Sync>,
    pub materializer: Arc<dyn Materializer>,
    decode: Arc<DecodeState>,
}
impl SnapshotEntry {
    pub fn validate_state(&self, bytes: &[u8]) -> Result<(), crate::EventCodecError> {
        (self.decode)(bytes).map(drop)
    }

    pub fn new<A, C>(revision: &str, schema: u32, codec: C, policy: SnapshotPolicy) -> Self
    where
        A: Aggregate + 'static,
        A::Event: Event,
        A::State: Send + Sync,
        C: SnapshotCodec<A> + 'static,
    {
        let codec: Arc<dyn SnapshotCodec<A>> = Arc::new(codec);
        Self {
            revision: revision.to_owned(),
            schema,
            policy,
            codec: Arc::new(codec.clone()),
            materializer: Arc::new(TypedMaterializer::<A>(PhantomData)),
            decode: Arc::new(move |bytes| {
                codec
                    .decode(bytes)
                    .map(|state| -> Box<dyn Any + Send + Sync> { Box::new(state) })
            }),
        }
    }
}

#[async_trait]
pub trait Materializer: Send + Sync {
    async fn create(
        &self,
        history: &dyn EventHistory,
        codecs: &AggregateCodecs,
        stream: &StreamId,
        version: StreamVersion,
    ) -> Result<StoredSnapshot, CommandExecutionError>;
    async fn reconstruct(
        &self,
        history: &dyn EventHistory,
        codecs: &AggregateCodecs,
        stream: &StreamId,
        version: Option<StreamVersion>,
        use_snapshots: bool,
    ) -> Result<crate::AggregateReconstruction, CommandExecutionError>;
    fn fixture_state(
        &self,
        codecs: &AggregateCodecs,
        stream: &StreamId,
        events: &[crate::RecordedEvent],
    ) -> Result<Vec<u8>, CommandExecutionError>;
}
struct TypedMaterializer<A>(PhantomData<fn() -> A>);
#[async_trait]
impl<A> Materializer for TypedMaterializer<A>
where
    A: Aggregate + 'static,
    A::Event: Event,
    A::State: Send + Sync,
{
    async fn create(
        &self,
        history: &dyn EventHistory,
        codecs: &AggregateCodecs,
        stream: &StreamId,
        version: StreamVersion,
    ) -> Result<StoredSnapshot, CommandExecutionError> {
        create::<A>(history, codecs, stream, version).await
    }
    async fn reconstruct(
        &self,
        history: &dyn EventHistory,
        codecs: &AggregateCodecs,
        stream: &StreamId,
        version: Option<StreamVersion>,
        use_snapshots: bool,
    ) -> Result<crate::AggregateReconstruction, CommandExecutionError> {
        let diagnostics = Arc::new(crate::RebuildDiagnostics::new(1));
        let mut effective = codecs.clone();
        effective.diagnostics = Some(diagnostics.clone());
        // Inspection cannot enqueue maintenance or change application-wide replay settings.
        effective.snapshot_requests = Arc::default();
        if !use_snapshots {
            effective.snapshot_store = None;
        }
        let codec = effective.resolve::<A>()?;
        let loaded = load::<A>(history, &effective, stream, codec.as_ref(), version, None).await?;
        let snapshot_codec = snapshot_codec::<A>(codecs, stream)?;
        Ok(crate::AggregateReconstruction {
            version: loaded.version,
            state: snapshot_codec.encode(loaded.aggregate.state())?,
            report: diagnostics
                .recent()
                .pop()
                .ok_or_else(|| crate::replay::invalid("missing rebuild diagnostics"))?,
        })
    }
    fn fixture_state(
        &self,
        codecs: &AggregateCodecs,
        stream: &StreamId,
        events: &[crate::RecordedEvent],
    ) -> Result<Vec<u8>, CommandExecutionError> {
        crate::executor::validate_history_from(stream, events, StreamVersion::ZERO)?;
        let codec = codecs.resolve::<A>()?;
        let decoded = events
            .iter()
            .map(|event| codec.decode(event))
            .collect::<Result<Vec<_>, _>>()?;
        let aggregate = AggregateInstance::<A>::rehydrate(stream.clone(), decoded);
        let codec = snapshot_codec::<A>(codecs, stream)?;
        let bytes = codec.encode(aggregate.state())?;
        drop(codec.decode(&bytes)?);
        Ok(bytes)
    }
}

fn snapshot_codec<'a, A: Aggregate + 'static>(
    codecs: &'a AggregateCodecs,
    stream: &StreamId,
) -> Result<&'a Arc<dyn SnapshotCodec<A>>, CommandExecutionError> {
    codecs
        .snapshots
        .get(stream.aggregate_type())
        .and_then(|entry| entry.codec.downcast_ref::<Arc<dyn SnapshotCodec<A>>>())
        .ok_or_else(|| crate::replay::invalid("aggregate has no compatible snapshot codec").into())
}

pub struct Loaded<A: Aggregate> {
    pub aggregate: AggregateInstance<A>,
    pub version: StreamVersion,
    pub checkpoint: Option<crate::HistoryCheckpoint>,
}

pub async fn load<A: Aggregate + 'static>(
    history: &dyn EventHistory,
    codecs: &AggregateCodecs,
    stream: &StreamId,
    codec: &dyn EventCodec<A>,
    through: Option<StreamVersion>,
    operation: Option<&str>,
) -> Result<Loaded<A>, CommandExecutionError> {
    let mut observation = Observation::new(codecs, stream, operation);
    observation.report.authority = history.history_authority().map(str::to_owned);
    observation.report.target_version = through.map(StreamVersion::value);
    let result = load_inner::<A>(
        history,
        codecs,
        stream,
        codec,
        through,
        &mut observation.report,
    )
    .await;
    observation.report.error = result.as_ref().err().map(ToString::to_string);
    result
}

async fn load_inner<A: Aggregate + 'static>(
    history: &dyn EventHistory,
    codecs: &AggregateCodecs,
    stream: &StreamId,
    codec: &dyn EventCodec<A>,
    through: Option<StreamVersion>,
    report: &mut RebuildReport,
) -> Result<Loaded<A>, CommandExecutionError> {
    let entry = codecs.snapshots.get(stream.aggregate_type());
    let lookup = Instant::now();
    let mut snapshot = select_snapshot::<A>(history, codecs, stream, through, report).await;
    report.snapshot_lookup_ns = crate::snapshots::nanos(lookup.elapsed());
    let history_started = Instant::now();
    let replay_result = if entry.is_none() && through.is_none() {
        history
            .load(stream)
            .await
            .map(|events| crate::ReplayHistory {
                events_read: events.len(),
                events,
                base_version: StreamVersion::ZERO,
                checkpoint: None,
            })
    } else {
        history
            .load_replay(
                stream,
                snapshot.as_ref().map(|value| &value.snapshot.checkpoint),
                through,
            )
            .await
    };
    let replay = match replay_result {
        Ok(replay) => replay,
        Err(error)
            if snapshot.is_some()
                && matches!(
                    error.kind(),
                    crate::EventStoreErrorKind::InvalidRequest
                        | crate::EventStoreErrorKind::CorruptHistory
                ) =>
        {
            snapshot = None;
            report.snapshot_status = SnapshotStatus::InvalidAnchor;
            history.load_replay(stream, None, through).await?
        }
        Err(error) => return Err(error.into()),
    };
    report.history_ns = crate::snapshots::nanos(history_started.elapsed());
    report.snapshot_version = snapshot
        .as_ref()
        .map_or(0, |value| value.snapshot.checkpoint.version);
    if replay.base_version.value() != report.snapshot_version {
        return Err(
            crate::replay::invalid("history returned an incompatible checkpoint base").into(),
        );
    }
    let version = replay.version();
    report.head_version = version.value();
    report.events_read = replay.events_read;
    report.event_bytes = replay.events.iter().fold(0_usize, |total, event| {
        total.saturating_add(event.payload().len())
    });
    let validation = Instant::now();
    let validated =
        crate::executor::validate_history_from(stream, &replay.events, replay.base_version);
    report.validation_ns = crate::snapshots::nanos(validation.elapsed());
    validated?;
    let aggregate = restore::<A>(stream, codec, snapshot, &replay.events, report)?;
    report.snapshot_recommended = entry.is_some_and(|entry| {
        entry.policy.recommends(
            version.value().saturating_sub(report.snapshot_version),
            history_started.elapsed(),
        )
    });
    queue_recommendation(history, codecs, stream, through, report, version);
    Ok(Loaded {
        aggregate,
        version,
        checkpoint: replay.checkpoint,
    })
}

fn restore<A: Aggregate + 'static>(
    stream: &StreamId,
    codec: &dyn EventCodec<A>,
    snapshot: Option<SelectedSnapshot>,
    events: &[crate::RecordedEvent],
    report: &mut RebuildReport,
) -> Result<AggregateInstance<A>, CommandExecutionError> {
    let decode = Instant::now();
    let restored = snapshot
        .map(|selected| {
            selected
                .state
                .downcast::<A::State>()
                .map(|state| *state)
                .map_err(|_| crate::replay::invalid("snapshot state belongs to another model"))
        })
        .transpose()?;
    let decoded = events
        .iter()
        .map(|event| codec.decode(event))
        .collect::<Result<Vec<_>, _>>();
    report.decode_ns = crate::snapshots::nanos(decode.elapsed());
    let decoded = decoded?;
    let apply = Instant::now();
    let aggregate = match restored {
        Some(state) => AggregateInstance::<A>::restore(stream.clone(), state, decoded),
        None => AggregateInstance::<A>::rehydrate(stream.clone(), decoded),
    };
    report.apply_ns = crate::snapshots::nanos(apply.elapsed());
    report.events_applied = events.len();
    Ok(aggregate)
}

fn queue_recommendation(
    history: &dyn EventHistory,
    codecs: &AggregateCodecs,
    stream: &StreamId,
    through: Option<StreamVersion>,
    report: &RebuildReport,
    version: StreamVersion,
) {
    if report.snapshot_recommended
        && through.is_none()
        && codecs.snapshot_store.is_some()
        && let Some(authority) = history.history_authority()
    {
        let mut requests = codecs
            .snapshot_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(request) = requests
            .iter_mut()
            .find(|request| &request.stream == stream && request.authority == authority)
        {
            request.version = request.version.max(version);
        } else if requests.len() < 128 {
            requests.push_back(crate::SnapshotRequest {
                authority: authority.to_owned(),
                stream: stream.clone(),
                version,
            });
        }
    }
}

struct SelectedSnapshot {
    snapshot: StoredSnapshot,
    state: Box<dyn Any + Send + Sync>,
}

async fn select_snapshot<A: Aggregate + 'static>(
    history: &dyn EventHistory,
    codecs: &AggregateCodecs,
    stream: &StreamId,
    through: Option<StreamVersion>,
    report: &mut RebuildReport,
) -> Option<SelectedSnapshot> {
    let (Some(entry), Some(store), Some(authority)) = (
        codecs.snapshots.get(stream.aggregate_type()),
        &codecs.snapshot_store,
        history.history_authority(),
    ) else {
        return None;
    };
    let key = SnapshotKey::new(authority, stream, &entry.revision, entry.schema);
    report.snapshot_status = SnapshotStatus::Miss;
    match tokio::time::timeout(
        std::time::Duration::from_secs(1),
        store.load(&key, through.unwrap_or(StreamVersion::new(u64::MAX))),
    )
    .await
    {
        Ok(Ok(Some(candidate))) => {
            if candidate.validate().is_err()
                || candidate
                    .checkpoint
                    .validate_for(authority, stream)
                    .is_err()
                || candidate.replay_revision != entry.revision
                || candidate.schema_version != entry.schema
                || through.is_some_and(|limit| candidate.version() > limit)
            {
                report.snapshot_status = SnapshotStatus::InvalidCheckpoint;
            } else {
                let started = Instant::now();
                let state = (entry.decode)(&candidate.state);
                report.snapshot_decode_ns = crate::snapshots::nanos(started.elapsed());
                if let Ok(state) = state
                    && entry.codec.is::<Arc<dyn SnapshotCodec<A>>>()
                {
                    report.snapshot_status = SnapshotStatus::Hit;
                    return Some(SelectedSnapshot {
                        snapshot: candidate,
                        state,
                    });
                }
                report.snapshot_status = SnapshotStatus::InvalidState;
            }
        }
        Ok(Ok(None)) => {}
        Ok(Err(_)) => report.snapshot_status = SnapshotStatus::Unavailable,
        Err(_) => report.snapshot_status = SnapshotStatus::Timeout,
    }
    None
}

struct Observation<'a> {
    diagnostics: Option<&'a crate::RebuildDiagnostics>,
    started: Instant,
    report: RebuildReport,
}
impl<'a> Observation<'a> {
    fn new(codecs: &'a AggregateCodecs, stream: &StreamId, operation: Option<&str>) -> Self {
        Self {
            diagnostics: codecs.diagnostics.as_deref(),
            started: Instant::now(),
            report: RebuildReport {
                aggregate_type: stream.aggregate_type().as_str().to_owned(),
                aggregate_id: stream.aggregate_id().as_str().to_owned(),
                operation_id: operation.map(str::to_owned),
                authority: None,
                replay_revision: codecs
                    .snapshots
                    .get(stream.aggregate_type())
                    .map(|entry| entry.revision.clone()),
                target_version: None,
                snapshot_version: 0,
                head_version: 0,
                events_applied: 0,
                events_read: 0,
                event_bytes: 0,
                total_ns: 0,
                snapshot_lookup_ns: 0,
                snapshot_decode_ns: 0,
                history_ns: 0,
                validation_ns: 0,
                decode_ns: 0,
                apply_ns: 0,
                snapshot_status: SnapshotStatus::Disabled,
                error: Some("cancelled".to_owned()),
                snapshot_recommended: false,
            },
        }
    }
}
impl Drop for Observation<'_> {
    fn drop(&mut self) {
        self.report.total_ns = crate::snapshots::nanos(self.started.elapsed());
        if let Some(diagnostics) = self.diagnostics {
            diagnostics.record(self.report.clone());
        }
    }
}

pub async fn create<A>(
    history: &dyn EventHistory,
    codecs: &AggregateCodecs,
    stream: &StreamId,
    version: StreamVersion,
) -> Result<StoredSnapshot, CommandExecutionError>
where
    A: Aggregate + 'static,
    A::Event: Event,
    A::State: Send + Sync,
{
    if stream.aggregate_type().as_str() != A::aggregate_type() {
        return Err(crate::replay::invalid("snapshot aggregate type mismatch").into());
    }
    let authority = history
        .history_authority()
        .ok_or_else(|| crate::replay::invalid("history adapter has no checkpoint authority"))?;
    let entry = codecs
        .snapshots
        .get(stream.aggregate_type())
        .ok_or_else(|| crate::replay::invalid("aggregate has no registered snapshot codec"))?;
    let store = codecs
        .snapshot_store
        .as_ref()
        .ok_or_else(|| crate::replay::invalid("snapshot storage is not configured"))?;
    let snapshot_codec = entry
        .codec
        .downcast_ref::<Arc<dyn SnapshotCodec<A>>>()
        .ok_or_else(|| {
            crate::replay::invalid("snapshot codec belongs to another aggregate model")
        })?;
    let codec = codecs.resolve::<A>()?;
    let snapshot = {
        let loaded =
            load::<A>(history, codecs, stream, codec.as_ref(), Some(version), None).await?;
        let checkpoint = loaded
            .checkpoint
            .ok_or_else(|| crate::replay::invalid("cannot snapshot version zero"))?;
        let state = snapshot_codec.encode(loaded.aggregate.state())?;
        // Verify that the codec can read its output before publishing a cache entry.
        drop(snapshot_codec.decode(&state)?);
        StoredSnapshot::new(entry.revision.clone(), entry.schema, checkpoint, state)?
    };
    let key = SnapshotKey::new(authority, stream, &entry.revision, entry.schema);
    store.save(&key, &snapshot).await?;
    Ok(snapshot)
}

pub async fn verify<A>(
    history: &dyn EventHistory,
    codecs: &AggregateCodecs,
    stream: &StreamId,
    version: StreamVersion,
) -> Result<bool, CommandExecutionError>
where
    A: Aggregate + 'static,
    A::Event: Event,
{
    let authority = history
        .history_authority()
        .ok_or_else(|| crate::replay::invalid("history adapter has no checkpoint authority"))?;
    let entry = codecs
        .snapshots
        .get(stream.aggregate_type())
        .ok_or_else(|| crate::replay::invalid("aggregate has no registered snapshot codec"))?;
    let store = codecs
        .snapshot_store
        .as_ref()
        .ok_or_else(|| crate::replay::invalid("snapshot storage is not configured"))?;
    let key = SnapshotKey::new(authority, stream, &entry.revision, entry.schema);
    let snapshot = store
        .load(&key, version)
        .await?
        .filter(|value| value.version() == version)
        .ok_or_else(|| crate::replay::invalid("no snapshot exists at the requested version"))?;
    snapshot.validate()?;
    snapshot.checkpoint.validate_for(authority, stream)?;
    if snapshot.replay_revision != entry.revision || snapshot.schema_version != entry.schema {
        return Err(crate::replay::invalid("snapshot replay revision or schema mismatch").into());
    }
    let snapshot_codec = entry
        .codec
        .downcast_ref::<Arc<dyn SnapshotCodec<A>>>()
        .ok_or_else(|| crate::replay::invalid("snapshot codec belongs to another model"))?;
    let codec = codecs.resolve::<A>()?;
    let mut original = AggregateCodecs::default();
    original.diagnostics.clone_from(&codecs.diagnostics);
    let rebuilt = load::<A>(
        history,
        &original,
        stream,
        codec.as_ref(),
        Some(version),
        None,
    )
    .await?;
    Ok(snapshot_codec.encode(rebuilt.aggregate.state())? == snapshot.state)
}
