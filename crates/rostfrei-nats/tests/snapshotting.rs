//! Version-selected snapshots through real storage and the real command worker.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
rostfrei::install_macro_support!();
mod release_contract_support;
use release_contract_support::*;

async fn seed(store: &NatsEventStore, id: &StreamId, count: u64) -> TestResult {
    for number in 1..=count {
        let operation = OperationId::new(format!("seed-{number}"))?;
        let commit = derive_commit_id(id, &operation);
        let event = <JsonEventCodec as EventCodec<Target>>::encode(
            &JsonEventCodec,
            &Changed {
                created: true,
                item: Some(number.to_string()),
            },
            derive_event_id(&commit, 0),
        )?;
        let expected = if number == 1 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(number.saturating_sub(1)))
        };
        store
            .append(
                id,
                expected,
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(number.to_string().as_bytes()),
                    vec![event],
                )?,
            )
            .await?;
    }
    Ok(())
}

#[tokio::test]
async fn selected_version_restores_only_the_tail_and_commands_preserve_history() -> TestResult {
    let fixture = Fixture::new("snapshots").await?;
    let store = Arc::new(
        fixture
            .split_event_store()
            .await?
            .as_ref()
            .clone()
            .with_snapshotting(true),
    );
    let id = stream("large")?;
    seed(&store, &id, 200).await?;
    let config = NatsReadModelConfig::new(&fixture.context, "aggregate-snapshots")?
        .with_storage_limits(16 * 1024 * 1024, 1024 * 1024)?;
    provision_read_model(&fixture.js, &config).await?;
    let snapshots = Arc::new(NatsSnapshotStore::connect(fixture.js.clone(), config.clone()).await?);
    let diagnostics = Arc::new(RebuildDiagnostics::new(32));
    let mut codecs = AggregateCodecs::default()
        .with_snapshot_store(snapshots)
        .with_rebuild_diagnostics(diagnostics.clone());
    codecs.register_json_snapshot::<Target>("target-v1", 1, SnapshotPolicy::Manual)?;
    let codecs = Arc::new(codecs);
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    let snapshot = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(150))
        .await?;
    assert_eq!(snapshot.version(), StreamVersion::new(150));
    assert!(
        executor
            .verify_snapshot::<Target>(&id, StreamVersion::new(150))
            .await?
    );
    assert_eq!(
        executor
            .rehydrate_at::<Target>(&id, StreamVersion::new(150))
            .await?
            .state()
            .items
            .len(),
        150
    );
    let aggregate = executor.rehydrate::<Target>(&id).await?;
    assert_eq!(aggregate.state().items.len(), 200);
    let report = diagnostics.recent().pop().ok_or("missing rebuild report")?;
    assert_eq!(report.snapshot_version, 150);
    assert_eq!(report.events_applied, 50);
    assert_eq!(
        report.events_read, 50,
        "the skipped prefix must not be transferred"
    );
    let mut processor = CommandProcessor::new(fixture.context.name().clone(), store.clone())
        .with_event_codecs(codecs);
    processor.register::<Act, _>(Handler(Arc::new(AtomicUsize::new(0))), map_rejection)?;
    let _worker = fixture.command_worker(Arc::new(processor)).await?;
    let bus = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let command = request("after-snapshot", "large", "add")?;
    assert_eq!(
        bus.dispatch(command.clone()).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(
        bus.dispatch(command).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(store.load(&id).await?.len(), 201);
    assert_eq!(
        executor.rehydrate::<Target>(&id).await?.state().items.len(),
        201
    );
    assert_eq!(store.audit_history(&id).await?.len(), 201);
    // A new adapter/registry sees the persisted checkpoint, not a process cache.
    let mut reopened = AggregateCodecs::default()
        .with_snapshot_store(Arc::new(
            NatsSnapshotStore::connect(fixture.js.clone(), config).await?,
        ))
        .with_rebuild_diagnostics(diagnostics.clone());
    reopened.register_json_snapshot::<Target>("target-v1", 1, SnapshotPolicy::Manual)?;
    CommandExecutor::with_codecs(store, Arc::new(reopened))
        .rehydrate::<Target>(&id)
        .await?;
    assert_eq!(
        diagnostics
            .recent()
            .pop()
            .ok_or("missing reopened report")?
            .events_applied,
        51
    );
    Ok(())
}

async fn setup(
    label: &str,
    policy: SnapshotPolicy,
) -> TestResult<(
    Fixture,
    Arc<NatsEventStore>,
    Arc<AggregateCodecs>,
    Arc<RebuildDiagnostics>,
)> {
    let fixture = Fixture::new(label).await?;
    let store = Arc::new(
        fixture
            .split_event_store()
            .await?
            .as_ref()
            .clone()
            .with_snapshotting(true),
    );
    let config = NatsReadModelConfig::new(&fixture.context, "snapshots")?;
    provision_read_model(&fixture.js, &config).await?;
    let snapshots = Arc::new(NatsSnapshotStore::connect(fixture.js.clone(), config).await?);
    let reports = Arc::new(RebuildDiagnostics::new(64));
    let mut codecs = AggregateCodecs::default()
        .with_snapshot_store(snapshots)
        .with_rebuild_diagnostics(reports.clone());
    codecs.register_json_snapshot::<Target>("v1", 1, policy)?;
    Ok((fixture, store, Arc::new(codecs), reports))
}

#[tokio::test]
async fn head_older_targets_and_strict_audits_preserve_versions() -> TestResult {
    let (_fixture, store, codecs, reports) =
        setup("snapshot-targets", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 10).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    let missing = stream("missing")?;
    assert!(
        executor
            .rehydrate_at::<Target>(&missing, StreamVersion::ZERO)
            .await?
            .state()
            .items
            .is_empty()
    );
    assert!(
        matches!(executor.create_snapshot::<Target>(&missing, StreamVersion::new(1)).await,
        Err(CommandExecutionError::Store(error)) if error.kind() == EventStoreErrorKind::InvalidRequest)
    );
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(10))
        .await?;
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(4))
        .await?;
    assert_eq!(
        executor.rehydrate::<Target>(&id).await?.state().items.len(),
        10
    );
    let report = reports.recent().pop().ok_or("report")?;
    assert_eq!(
        (
            report.snapshot_version,
            report.head_version,
            report.events_applied,
            report.events_read
        ),
        (10, 10, 0, 0)
    );
    assert_eq!(
        executor
            .rehydrate_at::<Target>(&id, StreamVersion::new(7))
            .await?
            .state()
            .items
            .len(),
        7
    );
    assert_eq!(reports.recent().pop().ok_or("report")?.events_applied, 3);
    assert!(
        executor
            .create_snapshot::<Target>(&id, StreamVersion::new(11))
            .await
            .is_err()
    );
    assert!(
        executor
            .create_snapshot::<Target>(&id, StreamVersion::ZERO)
            .await
            .is_err()
    );
    let strict =
        CommandExecutor::with_codecs(store.as_ref().clone().with_history_auditing(true), codecs);
    assert_eq!(
        strict.rehydrate::<Target>(&id).await?.state().items.len(),
        10
    );
    assert_eq!(reports.recent().pop().ok_or("report")?.events_read, 10);
    Ok(())
}

#[tokio::test]
async fn rejects_mid_commit_capture_and_retains_complete_history_after_session_replay() -> TestResult
{
    let (_fixture, store, codecs, _) = setup("snapshot-boundary", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    let operation = OperationId::new("multi")?;
    let commit = derive_commit_id(&id, &operation);
    let events = (0..3)
        .map(|ordinal| {
            <JsonEventCodec as EventCodec<Target>>::encode(
                &JsonEventCodec,
                &Changed {
                    created: true,
                    item: Some(ordinal.to_string()),
                },
                derive_event_id(&commit, ordinal),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    store
        .append(
            &id,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest(b"multi"),
                events,
            )?,
        )
        .await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    assert!(
        executor
            .create_snapshot::<Target>(&id, StreamVersion::new(2))
            .await
            .is_err()
    );
    let snapshot = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(3))
        .await?;
    let session = store.begin_command().await?;
    let replay = session
        .load_replay(&id, Some(&snapshot.checkpoint), None)
        .await?;
    assert_eq!(replay.version(), StreamVersion::new(3));
    assert!(replay.events.is_empty());
    assert_eq!(
        session.load(&id).await?.len(),
        3,
        "load() must retain its full-history contract"
    );
    Ok(())
}

#[tokio::test]
async fn policy_requests_are_coalesced_and_materialized_outside_loads() -> TestResult {
    let (_fixture, store, codecs, reports) = setup(
        "snapshot-policy",
        SnapshotPolicy::EveryEvents(std::num::NonZeroU64::new(5).ok_or("nonzero")?),
    )
    .await?;
    let id = stream("one")?;
    seed(&store, &id, 10).await?;
    let executor = CommandExecutor::with_codecs(store, codecs.clone());
    for _ in 0..3 {
        executor.rehydrate::<Target>(&id).await?;
    }
    assert!(reports.recent().pop().ok_or("report")?.snapshot_recommended);
    assert_eq!(codecs.pending_snapshots().len(), 1);
    let jobs = executor.run_snapshot_jobs(1).await;
    assert_eq!(jobs.len(), 1);
    assert!(jobs.into_iter().all(|result| result.is_ok()));
    executor.rehydrate::<Target>(&id).await?;
    assert_eq!(reports.recent().pop().ok_or("report")?.events_applied, 0);
    assert!(codecs.pending_snapshots().is_empty());
    Ok(())
}

struct BrokenSnapshotStore;
#[async_trait::async_trait]
impl SnapshotStore for BrokenSnapshotStore {
    async fn load(
        &self,
        _: &SnapshotKey,
        _: StreamVersion,
    ) -> Result<Option<StoredSnapshot>, EventStoreError> {
        Err(EventStoreError::new(
            EventStoreErrorKind::Unavailable,
            "snapshot service unavailable",
        ))
    }
    async fn save(&self, _: &SnapshotKey, _: &StoredSnapshot) -> Result<(), EventStoreError> {
        Err(EventStoreError::new(
            EventStoreErrorKind::Unavailable,
            "snapshot service unavailable",
        ))
    }
}

#[tokio::test]
async fn cache_failures_and_replay_revision_changes_fall_back_to_original_events() -> TestResult {
    let (fixture, store, codecs, reports) =
        setup("snapshot-fallback", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(6))
        .await?;
    let config = NatsReadModelConfig::new(&fixture.context, "snapshots")?;
    let mut newer = AggregateCodecs::default()
        .with_snapshot_store(Arc::new(
            NatsSnapshotStore::connect(fixture.js.clone(), config).await?,
        ))
        .with_rebuild_diagnostics(reports.clone());
    newer.register_json_snapshot::<Target>("v2", 1, SnapshotPolicy::Manual)?;
    CommandExecutor::with_codecs(store.clone(), Arc::new(newer))
        .rehydrate::<Target>(&id)
        .await?;
    assert_eq!(reports.recent().pop().ok_or("report")?.events_applied, 8);
    let mut broken = AggregateCodecs::default()
        .with_snapshot_store(Arc::new(BrokenSnapshotStore))
        .with_rebuild_diagnostics(reports.clone());
    broken.register_json_snapshot::<Target>("v1", 1, SnapshotPolicy::Manual)?;
    let executor = CommandExecutor::with_codecs(store, Arc::new(broken));
    assert_eq!(
        executor.rehydrate::<Target>(&id).await?.state().items.len(),
        8
    );
    assert_eq!(
        reports.recent().pop().ok_or("report")?.snapshot_status,
        SnapshotStatus::Unavailable
    );
    assert!(
        executor
            .create_snapshot::<Target>(&id, StreamVersion::new(8))
            .await
            .is_err()
    );
    Ok(())
}

struct FixedSnapshot(StoredSnapshot);
#[async_trait::async_trait]
impl SnapshotStore for FixedSnapshot {
    async fn load(
        &self,
        _: &SnapshotKey,
        _: StreamVersion,
    ) -> Result<Option<StoredSnapshot>, EventStoreError> {
        Ok(Some(self.0.clone()))
    }
    async fn save(&self, _: &SnapshotKey, _: &StoredSnapshot) -> Result<(), EventStoreError> {
        Ok(())
    }
}

#[tokio::test]
async fn damaged_state_and_wrong_scope_caches_never_replace_original_history() -> TestResult {
    let (_fixture, store, codecs, reports) =
        setup("snapshot-invalid", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    let original = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(6))
        .await?;
    let mut bad_checksum = original.clone();
    bad_checksum.state.push(255);
    let bad_state = StoredSnapshot::new(
        original.replay_revision.clone(),
        1,
        original.checkpoint.clone(),
        b"bad json".to_vec(),
    )?;
    let mut other_scope = original.checkpoint.clone();
    other_scope.authority = "another-authority".to_owned();
    let wrong_scope = StoredSnapshot::new(
        original.replay_revision.clone(),
        1,
        other_scope,
        original.state.clone(),
    )?;
    for snapshot in [bad_checksum, bad_state, wrong_scope] {
        let mut codecs = AggregateCodecs::default()
            .with_snapshot_store(Arc::new(FixedSnapshot(snapshot)))
            .with_rebuild_diagnostics(reports.clone());
        codecs.register_json_snapshot::<Target>("v1", 1, SnapshotPolicy::Manual)?;
        let restored = CommandExecutor::with_codecs(store.clone(), Arc::new(codecs))
            .rehydrate::<Target>(&id)
            .await?;
        assert_eq!(restored.state().items.len(), 8);
        assert_eq!(reports.recent().pop().ok_or("report")?.events_applied, 8);
    }
    let wrong_semantics = StoredSnapshot::new(
        "v1".to_owned(),
        1,
        original.checkpoint.clone(),
        br#"{"exists":true,"items":[]}"#.to_vec(),
    )?;
    let mut codecs =
        AggregateCodecs::default().with_snapshot_store(Arc::new(FixedSnapshot(wrong_semantics)));
    codecs.register_json_snapshot::<Target>("v1", 1, SnapshotPolicy::Manual)?;
    assert!(
        !CommandExecutor::with_codecs(store, Arc::new(codecs))
            .verify_snapshot::<Target>(&id, StreamVersion::new(6))
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_publications_do_not_regress_latest_or_overwrite_an_existing_version()
-> TestResult {
    let (fixture, store, codecs, reports) =
        setup("snapshot-publish", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 10).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    let (older, latest) = tokio::join!(
        executor.create_snapshot::<Target>(&id, StreamVersion::new(4)),
        executor.create_snapshot::<Target>(&id, StreamVersion::new(10))
    );
    older?;
    let latest = latest?;
    let duplicate = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(10))
        .await?;
    assert_eq!(duplicate, latest);
    let snapshots = NatsSnapshotStore::connect(
        fixture.js.clone(),
        NatsReadModelConfig::new(&fixture.context, "snapshots")?,
    )
    .await?;
    let conflict = StoredSnapshot::new(
        "v1".to_owned(),
        1,
        latest.checkpoint.clone(),
        b"{}".to_vec(),
    )?;
    let key = SnapshotKey::new(store.authority().as_str(), &id, "v1", 1);
    assert!(snapshots.save(&key, &conflict).await.is_err());
    executor.rehydrate::<Target>(&id).await?;
    assert_eq!(reports.recent().pop().ok_or("report")?.snapshot_version, 10);
    Ok(())
}

struct GuardedHandler;
#[async_trait::async_trait]
impl CommandHandler<Act> for GuardedHandler {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut target = execution.load::<Target>(&command.target).await?;
        let other = execution.load::<Target>("empty").await?;
        assert!(!other.state().exists);
        assert_eq!(target.base_version(), StreamVersion::new(8));
        if command.action == "reject" {
            return Ok(CommandDecision::Rejected("target-not-found"));
        }
        if command.action == "write" {
            target.raise(Changed {
                created: false,
                item: Some("last".to_owned()),
            });
        }
        Ok(CommandDecision::Accepted)
    }
}

#[tokio::test]
async fn checkpoint_only_participants_keep_read_guards_and_completed_rejections() -> TestResult {
    let (fixture, store, codecs, _) = setup("snapshot-guards", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(8))
        .await?;
    let mut processor = CommandProcessor::new(fixture.context.name().clone(), store.clone())
        .with_event_codecs(codecs);
    processor.register::<Act, _>(GuardedHandler, map_rejection)?;
    let _worker = fixture.command_worker(Arc::new(processor)).await?;
    let bus = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    for action in ["read", "reject", "write"] {
        let command = request(action, "one", action)?;
        let first = bus.dispatch(command.clone()).await?;
        assert_eq!(bus.dispatch(command).await?.response(), first.response());
        if action == "reject" {
            assert!(rejected(first.response().outcome()));
        } else {
            assert_eq!(
                first.response().outcome(),
                &CommandResponseOutcome::Accepted
            );
        }
    }
    assert_eq!(store.audit_history(&id).await?.len(), 9);
    Ok(())
}

struct RacingHandler {
    first: AtomicBool,
    loaded: Arc<tokio::sync::Semaphore>,
    resume: Arc<tokio::sync::Semaphore>,
}
#[async_trait::async_trait]
impl CommandHandler<Act> for RacingHandler {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut target = execution.load::<Target>(&command.target).await?;
        if self.first.swap(false, Ordering::SeqCst) {
            self.loaded.add_permits(1);
            self.resume
                .acquire()
                .await
                .map_err(|error| {
                    EventStoreError::new(EventStoreErrorKind::Unavailable, error.to_string())
                })?
                .forget();
        }
        target.raise(Changed {
            created: false,
            item: Some("command".to_owned()),
        });
        Ok(CommandDecision::Accepted)
    }
}

#[tokio::test]
async fn concurrent_writer_forces_a_fresh_tail_and_retries_at_the_new_head() -> TestResult {
    let (fixture, store, codecs, reports) = setup("snapshot-race", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    CommandExecutor::with_codecs(store.clone(), codecs.clone())
        .create_snapshot::<Target>(&id, StreamVersion::new(8))
        .await?;
    let loaded = Arc::new(tokio::sync::Semaphore::new(0));
    let resume = Arc::new(tokio::sync::Semaphore::new(0));
    let mut processor = CommandProcessor::new(fixture.context.name().clone(), store.clone())
        .with_event_codecs(codecs);
    processor.register::<Act, _>(
        RacingHandler {
            first: AtomicBool::new(true),
            loaded: loaded.clone(),
            resume: resume.clone(),
        },
        map_rejection,
    )?;
    let _worker = fixture.command_worker(Arc::new(processor)).await?;
    let bus = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let writer = async {
        loaded.acquire().await?.forget();
        let operation = OperationId::new("racing-write")?;
        let commit = derive_commit_id(&id, &operation);
        let event = <JsonEventCodec as EventCodec<Target>>::encode(
            &JsonEventCodec,
            &Changed {
                created: false,
                item: Some("concurrent".to_owned()),
            },
            derive_event_id(&commit, 0),
        )?;
        store
            .append(
                &id,
                ExpectedVersion::Exact(StreamVersion::new(8)),
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(b"race"),
                    vec![event],
                )?,
            )
            .await?;
        resume.add_permits(1);
        TestResult::Ok(())
    };
    let (result, writer) = tokio::time::timeout(WAIT, async {
        tokio::join!(
            bus.dispatch(request("racing-command", "one", "write").unwrap()),
            writer
        )
    })
    .await?;
    writer?;
    assert_eq!(
        result?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    let attempts: Vec<_> = reports
        .recent()
        .into_iter()
        .filter(|report| report.operation_id.as_deref() == Some("racing-command"))
        .collect();
    assert_eq!(
        attempts
            .iter()
            .map(|report| report.head_version)
            .collect::<Vec<_>>(),
        [8, 9]
    );
    assert_eq!(store.load(&id).await?.len(), 10);
    Ok(())
}

async fn seed_many(store: &NatsEventStore, id: &StreamId, count: u64, boundary: u64) -> TestResult {
    let mut version = 0_u64;
    while version < count {
        let end = version
            .saturating_add(u64::try_from(MAX_EVENTS_PER_BATCH)?)
            .min(count)
            .min(if version < boundary { boundary } else { count });
        let operation = OperationId::new(format!("bulk-{version}"))?;
        let commit = derive_commit_id(id, &operation);
        let events = (version..end)
            .enumerate()
            .map(|(ordinal, number)| {
                <JsonEventCodec as EventCodec<Target>>::encode(
                    &JsonEventCodec,
                    &Changed {
                        created: true,
                        item: Some(number.to_string()),
                    },
                    derive_event_id(&commit, u32::try_from(ordinal).unwrap()),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let expected = if version == 0 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(version))
        };
        store
            .append(
                id,
                expected,
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(version.to_string()),
                    events,
                )?,
            )
            .await?;
        version = end;
    }
    Ok(())
}

#[tokio::test]
async fn whole_command_and_exact_replay_transfer_a_bounded_tail() -> TestResult {
    let (fixture, store, codecs, reports) =
        setup("snapshot-whole-cost", SnapshotPolicy::Manual).await?;
    let id = stream("large")?;
    seed_many(&store, &id, 10_000, 9_950).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(9_950))
        .await?;
    let command = Act {
        target: "large".to_owned(),
        action: "add".to_owned(),
        item: "after-snapshot".to_owned(),
    };
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("whole-command")?,
        ContentFingerprint::digest(b"whole-command"),
    )
    .with_bounded_context(fixture.context.name().clone());
    let handler = Handler(Arc::new(AtomicUsize::new(0)));
    for exact_replay in [false, true] {
        let client = fixture.js.client();
        client.flush().await?;
        let messages = client.statistics().in_messages.load(Ordering::Relaxed);
        let bytes = client.statistics().in_bytes.load(Ordering::Relaxed);
        let result = executor
            .execute(&handler, metadata.clone(), &command)
            .await?;
        assert!(matches!(result, CommandOutcome::Accepted(_)));
        client.flush().await?;
        let messages = client
            .statistics()
            .in_messages
            .load(Ordering::Relaxed)
            .saturating_sub(messages);
        let bytes = client
            .statistics()
            .in_bytes
            .load(Ordering::Relaxed)
            .saturating_sub(bytes);
        assert!(
            messages < 500,
            "exact={exact_replay}: whole command read {messages} messages from a 10,000-event history"
        );
        assert!(
            bytes < 1_000_000,
            "exact={exact_replay}: whole command read {bytes} bytes"
        );
        eprintln!(
            "10,000 events / checkpoint 9,950 / exact={exact_replay}: {messages} received messages, {bytes} received bytes"
        );
    }
    let load = reports
        .recent()
        .into_iter()
        .find(|report| report.operation_id.as_deref() == Some("whole-command"))
        .ok_or("command report")?;
    assert_eq!(
        (load.snapshot_version, load.events_read, load.events_applied),
        (9_950, 50, 50)
    );
    assert_eq!(store.load(&id).await?.len(), 10_001);
    Ok(())
}

#[tokio::test]
async fn skipped_direct_operation_identities_still_fence_command_writes() -> TestResult {
    let (_fixture, store, codecs, _) =
        setup("snapshot-old-identity", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    let snapshot = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(8))
        .await?;
    let session = store.append_session().await?;
    session
        .load_replay(&id, Some(&snapshot.checkpoint), None)
        .await?;
    let operation = OperationId::new("seed-1")?;
    let commit = derive_commit_id(&id, &operation);
    let event = <JsonEventCodec as EventCodec<Target>>::encode(
        &JsonEventCodec,
        &Changed {
            created: false,
            item: Some("duplicate".to_owned()),
        },
        derive_event_id(&commit, 0),
    )?;
    let fingerprint = ContentFingerprint::digest(b"reuse");
    let batch = EventBatch::new(commit, operation.clone(), fingerprint, vec![event])?;
    let result = session
        .append_transaction(EventTransaction::new(
            operation,
            fingerprint,
            vec![TransactionParticipant::new(
                id.clone(),
                ExpectedVersion::Exact(StreamVersion::new(8)),
                Some(batch),
            )],
        ))
        .await;
    assert!(matches!(result, Err(error) if error.kind() == EventStoreErrorKind::IdentityConflict));
    assert_eq!(store.load(&id).await?.len(), 8);
    Ok(())
}

#[tokio::test]
async fn storage_recreation_fences_old_checkpoints_and_old_handles() -> TestResult {
    let (fixture, store, codecs, reports) =
        setup("snapshot-incarnation", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(8))
        .await?;
    fixture
        .js
        .delete_stream(store.config().stream_name())
        .await?;
    fixture
        .js
        .delete_stream(store.config().outcome_state_stream_name())
        .await?;
    let recreated = Arc::new(
        fixture
            .split_event_store()
            .await?
            .as_ref()
            .clone()
            .with_snapshotting(true),
    );
    seed(&recreated, &id, 3).await?;
    assert!(executor.rehydrate::<Target>(&id).await.is_err());
    assert!(
        executor
            .create_snapshot::<Target>(&id, StreamVersion::new(3))
            .await
            .is_err()
    );
    let fresh = CommandExecutor::with_codecs(recreated, codecs);
    assert_eq!(fresh.rehydrate::<Target>(&id).await?.state().items.len(), 3);
    assert_eq!(
        reports.recent().pop().ok_or("report")?.snapshot_status,
        SnapshotStatus::Miss
    );
    Ok(())
}
