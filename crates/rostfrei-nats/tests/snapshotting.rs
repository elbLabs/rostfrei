//! Version-selected snapshots through real storage and the real command worker.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
rostfrei_runtime::install_macro_support!();
mod release_contract_support;
#[allow(dead_code)]
mod transaction_support;
use release_contract_support::*;
use transaction_support::transaction;

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
        let batch = EventBatch::new(
            commit,
            operation,
            ContentFingerprint::digest(number.to_string().as_bytes()),
            vec![event],
        )?;
        if store.config().receipt_backed_history() {
            store
                .append_transaction(transaction(id, expected, batch))
                .await?;
        } else {
            store.append_fixture(id, expected, batch).await?;
        }
    }
    Ok(())
}

async fn snapshot_command_worker(
    fixture: &Fixture,
    processor: Arc<CommandProcessor>,
) -> TestResult<(Worker, CommandBus)> {
    // Replay and the gated competing write must fit before processing cancellation;
    // reserve the rest of the scenario budget for delivery and durable responses.
    let config = ConsumerConfig::new(
        fixture.context.consumer_name("act", 1)?,
        fixture.context.durable_name("act", 1)?,
        fixture.context.command_address("act")?,
        WAIT,
        WAIT.checked_div(2).unwrap(),
        2,
        1000,
    )?;
    provision_durable_consumer(&fixture.js, fixture.messaging.topology(), &config).await?;
    let factory =
        NatsConsumerFactory::new(fixture.js.clone(), fixture.messaging.topology().clone());
    let consumer =
        <NatsConsumerFactory as MessageConsumerFactory<CommandAddress>>::create(&factory, config)?;
    let adapter = Arc::new(fixture.adapter.as_ref().clone().with_response_timeout(WAIT));
    let handler = Arc::new(adapter.command_handler(processor));
    let worker = Worker(tokio::spawn(async move { consumer.run(handler).await }));
    Ok((worker, CommandBus::new(fixture.context.clone(), adapter)))
}

#[tokio::test]
async fn selected_version_restores_only_the_tail_and_commands_preserve_history() -> TestResult {
    let fixture = Fixture::new("snapshots").await?;
    let store = snapshot_store(&fixture, true).await?;
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
    let (_worker, bus) = snapshot_command_worker(&fixture, Arc::new(processor)).await?;
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
    setup_profile(label, policy, true).await
}

async fn snapshot_store(
    fixture: &Fixture,
    receipt_backed: bool,
) -> TestResult<Arc<NatsEventStore>> {
    let mut config = NatsEventStoreConfig::for_bounded_context(&fixture.context)?
        .with_fresh_split_outcome_epoch("private-split-contract-fixture")?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?;
    if receipt_backed {
        config = config.with_receipt_backed_history()?;
    }
    provision_event_store(&fixture.js, &config).await?;
    Ok(Arc::new(
        NatsEventStore::connect(fixture.js.clone(), config)
            .await?
            .with_snapshotting(true),
    ))
}

async fn setup_profile(
    label: &str,
    policy: SnapshotPolicy,
    receipt_backed: bool,
) -> TestResult<(
    Fixture,
    Arc<NatsEventStore>,
    Arc<AggregateCodecs>,
    Arc<RebuildDiagnostics>,
)> {
    setup_in_scope(
        label,
        policy,
        receipt_backed,
        if receipt_backed {
            TrafficScope::Normal
        } else {
            TrafficScope::Test
        },
    )
    .await
}

async fn setup_in_scope(
    label: &str,
    policy: SnapshotPolicy,
    receipt_backed: bool,
    scope: TrafficScope,
) -> TestResult<(
    Fixture,
    Arc<NatsEventStore>,
    Arc<AggregateCodecs>,
    Arc<RebuildDiagnostics>,
)> {
    let mut fixture = Fixture::new(label).await?;
    if scope == TrafficScope::Test {
        fixture.context = fixture
            .context
            .application()
            .bounded_context_in_scope(TrafficScope::Test, fixture.context.name().as_str())?;
        fixture.messaging = ApplicationMessagingConfig::new_in_scope(
            fixture.context.application(),
            TrafficScope::Test,
        )?
        .with_max_bytes(64 * 1024 * 1024)?;
        provision_application_messaging(&fixture.js, &fixture.messaging).await?;
        fixture.adapter = Arc::new(
            NatsMessagingAdapter::new(
                NatsPublisher::new(fixture.js.clone(), fixture.messaging.topology().clone()),
                NatsCommandResponseReader::new(
                    fixture.js.clone(),
                    fixture.messaging.topology().clone(),
                ),
            )
            .with_response_timeout(Duration::from_secs(3)),
        );
    }
    let store = snapshot_store(&fixture, receipt_backed).await?;
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
async fn unqualified_history_replays_full_prefix_for_real_commands() -> TestResult {
    let (fixture, store, codecs, reports) =
        setup_profile("snapshot-unqualified", SnapshotPolicy::Manual, false).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    assert!(
        store
            .load_transaction_receipt(&OperationId::new("seed-1")?)
            .await?
            .is_none()
    );
    CommandExecutor::with_codecs(store.clone(), codecs.clone())
        .create_snapshot::<Target>(&id, StreamVersion::new(6))
        .await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut processor = CommandProcessor::new(fixture.context.name().clone(), store.clone())
        .with_event_codecs(codecs);
    processor.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let (_worker, bus) = snapshot_command_worker(&fixture, Arc::new(processor)).await?;
    let command = request("unqualified-command", "one", "add")?;
    assert_eq!(
        bus.dispatch(command.clone()).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(
        bus.dispatch(command).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.load(&id).await?.len(), 9);
    let report = reports
        .recent()
        .into_iter()
        .find(|report| report.operation_id.as_deref() == Some("unqualified-command"))
        .ok_or("command rebuild report")?;
    assert_eq!(report.snapshot_version, 6);
    assert_eq!(report.events_applied, 2);
    assert_eq!(
        report.events_read, 8,
        "unqualified histories must validate the whole prefix, even for a real command"
    );
    // The preceding command has now made this a mixed fixture/transaction history.
    bus.dispatch(request("mixed-command", "one", "add")?)
        .await?;
    let mixed = reports
        .recent()
        .into_iter()
        .find(|report| report.operation_id.as_deref() == Some("mixed-command"))
        .ok_or("mixed history report")?;
    assert_eq!(
        (
            mixed.snapshot_version,
            mixed.events_read,
            mixed.events_applied
        ),
        (6, 9, 3)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(store.load(&id).await?.len(), 10);
    Ok(())
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
        .append_transaction(transaction(
            &id,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest(b"multi"),
                events,
            )?,
        ))
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
    let mut bad_locator = original.checkpoint.clone();
    let mut position: serde_json::Value = serde_json::from_slice(&bad_locator.position)?;
    position["sequence"] = serde_json::json!(999_999);
    bad_locator.position = serde_json::to_vec(&position)?;
    assert_eq!(
        store
            .load_replay(&id, Some(&bad_locator), None)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::InvalidRequest,
        "a cache-supplied missing native position is not authoritative corruption"
    );
    let bad_locator = StoredSnapshot::new(
        original.replay_revision.clone(),
        1,
        bad_locator,
        original.state.clone(),
    )?;
    let mut wrong_anchor = original.checkpoint.clone();
    let mut position: serde_json::Value = serde_json::from_slice(&wrong_anchor.position)?;
    position["digest"] = serde_json::json!("wrong cache digest");
    wrong_anchor.position = serde_json::to_vec(&position)?;
    let wrong_anchor = StoredSnapshot::new(
        original.replay_revision.clone(),
        1,
        wrong_anchor,
        original.state.clone(),
    )?;
    for snapshot in [
        bad_checksum,
        bad_state,
        wrong_scope,
        bad_locator,
        wrong_anchor,
    ] {
        let mut codecs = AggregateCodecs::default()
            .with_snapshot_store(Arc::new(FixedSnapshot(snapshot)))
            .with_rebuild_diagnostics(reports.clone());
        codecs.register_json_snapshot::<Target>("v1", 1, SnapshotPolicy::Manual)?;
        let restored = CommandExecutor::with_codecs(store.clone(), Arc::new(codecs))
            .rehydrate::<Target>(&id)
            .await?;
        assert_eq!(restored.state().items.len(), 8);
        let report = reports.recent().pop().ok_or("report")?;
        assert_eq!((report.events_read, report.events_applied), (8, 8));
        assert!(report.snapshot_warning.is_some());
        assert!(report.error.is_none());
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
    let (_worker, bus) = snapshot_command_worker(&fixture, Arc::new(processor)).await?;
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
    let (_worker, bus) = snapshot_command_worker(&fixture, Arc::new(processor)).await?;
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
            .append_transaction(transaction(
                &id,
                ExpectedVersion::Exact(StreamVersion::new(8)),
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(b"race"),
                    vec![event],
                )?,
            ))
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
            .saturating_add(u64::try_from(MAX_TRANSACTION_ITEMS.saturating_sub(1))?)
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
            .append_transaction(transaction(
                id,
                expected,
                EventBatch::new(
                    commit,
                    operation,
                    ContentFingerprint::digest(version.to_string()),
                    events,
                )?,
            ))
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
async fn skipped_command_operation_identities_still_fence_command_writes() -> TestResult {
    let (_fixture, store, codecs, _) =
        setup("snapshot-old-identity", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    let snapshot = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(8))
        .await?;
    let session = store.append_session().await?;
    let replay = session
        .load_replay(&id, Some(&snapshot.checkpoint), None)
        .await?;
    assert!(replay.events.is_empty());
    assert_eq!(replay.events_read, 0);
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
    let recreated = snapshot_store(&fixture, true).await?;
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

fn fixture_events(id: &StreamId, operation: &str, count: u32) -> TestResult<EventBatch> {
    let operation = OperationId::new(operation)?;
    let commit = derive_commit_id(id, &operation);
    let events = (0..count)
        .map(|ordinal| {
            <JsonEventCodec as EventCodec<Target>>::encode(
                &JsonEventCodec,
                &Changed {
                    created: true,
                    item: Some(format!("fixture-{ordinal}")),
                },
                derive_event_id(&commit, ordinal),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventBatch::new(
        commit,
        operation,
        ContentFingerprint::digest(b"fixture"),
        events,
    )?)
}

#[tokio::test]
async fn receipt_profile_snapshots_preserve_complete_mixed_test_fixtures() -> TestResult {
    let (fixture, store, codecs, reports) = setup_in_scope(
        "snapshot-fixture",
        SnapshotPolicy::Manual,
        true,
        TrafficScope::Test,
    )
    .await?;
    let id = stream("one")?;
    store
        .append_fixture(
            &id,
            ExpectedVersion::NoStream,
            fixture_events(&id, "prefix", 2)?,
        )
        .await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    let error = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(1))
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::Store(error) if error.kind() == EventStoreErrorKind::InvalidRequest)
    );
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(2))
        .await?;
    assert_eq!(
        CommandExecutor::with_codecs(
            store.as_ref().clone().with_snapshotting(false),
            codecs.clone()
        )
        .rehydrate::<Target>(&id)
        .await?
        .state()
        .items
        .len(),
        2
    );
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("application-command")?,
        ContentFingerprint::digest(b"command"),
    )
    .with_bounded_context(fixture.context.name().clone());
    assert!(matches!(
        executor
            .execute(
                &Handler(Arc::new(AtomicUsize::new(0))),
                metadata,
                &Act {
                    target: "one".to_owned(),
                    action: "add".to_owned(),
                    item: "command".to_owned()
                },
            )
            .await?,
        CommandOutcome::Accepted(_)
    ));
    store
        .append_fixture(
            &id,
            ExpectedVersion::Exact(StreamVersion::new(3)),
            fixture_events(&id, "tail", 2)?,
        )
        .await?;
    let restored = executor.rehydrate::<Target>(&id).await?;
    assert_eq!(
        restored.state().items,
        [
            "fixture-0",
            "fixture-1",
            "command",
            "fixture-0",
            "fixture-1"
        ]
    );
    let report = reports.recent().pop().ok_or("report")?;
    assert_eq!(
        (
            report.snapshot_version,
            report.events_read,
            report.events_applied
        ),
        (2, 5, 3)
    );
    let error = executor
        .create_snapshot::<Target>(&id, StreamVersion::new(4))
        .await
        .unwrap_err();
    assert!(
        matches!(error, CommandExecutionError::Store(error) if error.kind() == EventStoreErrorKind::InvalidRequest)
    );
    executor
        .create_snapshot::<Target>(&id, StreamVersion::new(5))
        .await?;
    assert!(
        executor
            .verify_snapshot::<Target>(&id, StreamVersion::new(5))
            .await?
    );

    Ok(())
}

struct TwoParticipants;
#[async_trait::async_trait]
impl CommandHandler<Act> for TwoParticipants {
    type Rejection = &'static str;
    async fn handle(
        &self,
        _: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        for id in ["a-primary", "z-secondary"] {
            execution.load::<Target>(id).await?.raise(Changed {
                created: true,
                item: Some("conflicting-command".to_owned()),
            });
        }
        Ok(CommandDecision::Accepted)
    }
}

async fn expire_fixture_dedup(
    fixture: &Fixture,
    store: &NatsEventStore,
    id: &StreamId,
    batch: &EventBatch,
) -> TestResult {
    let journal = fixture.js.get_stream(store.config().stream_name()).await?;
    let original = journal.cached_info().config.clone();
    let subject = store
        .config()
        .event_subject(id.aggregate_type().as_str(), id.aggregate_id().as_str());
    let mut headers = async_nats::HeaderMap::new();
    headers.insert(
        "Nats-Msg-Id",
        batch
            .events()
            .first()
            .ok_or("empty fixture batch")?
            .event_id()
            .as_str(),
    );
    headers.insert("Nats-Expected-Last-Subject-Sequence", "0");
    let mut short = original.clone();
    short.duplicate_window = Duration::from_millis(100);
    fixture.js.update_stream(short).await?;
    // Observe expiration through a failing CAS: no probe event or new dedup ID
    // is persisted. Restore the required policy before exercising Rostfrei.
    tokio::time::timeout(WAIT, async {
        loop {
            match fixture
                .js
                .publish_with_headers(subject.clone(), headers.clone(), "{}".into())
                .await?
                .await
            {
                Ok(ack) => assert!(ack.duplicate, "the probe must never append"),
                Err(error) => {
                    assert_eq!(
                        error.kind(),
                        async_nats::jetstream::context::PublishErrorKind::WrongLastSequence
                    );
                    return TestResult::Ok(());
                }
            }
        }
    })
    .await??;
    fixture.js.update_stream(original).await?;
    assert_eq!(
        fixture.messages(store.config().stream_name()).await?,
        journal.cached_info().state.messages
    );
    Ok(())
}

#[tokio::test]
async fn secondary_fixture_prefix_fences_commands_after_snapshot_mode_changes() -> TestResult {
    let (fixture, store, codecs, reports) = setup_in_scope(
        "snapshot-fixture-identity",
        SnapshotPolicy::Manual,
        true,
        TrafficScope::Test,
    )
    .await?;
    let secondary = stream("z-secondary")?;
    let primary = stream("a-primary")?;
    let fixture_batch = fixture_events(&secondary, "fixture-F", 1)?;
    let full_store = store.as_ref().clone().with_snapshotting(false);
    full_store
        .append_fixture(&secondary, ExpectedVersion::NoStream, fixture_batch.clone())
        .await?;
    full_store
        .append_transaction(transaction(
            &secondary,
            ExpectedVersion::Exact(StreamVersion::new(1)),
            fixture_events(&secondary, "transaction-G", 1)?,
        ))
        .await?;
    CommandExecutor::with_codecs(full_store, codecs.clone())
        .create_snapshot::<Target>(&secondary, StreamVersion::new(2))
        .await?;
    expire_fixture_dedup(&fixture, &store, &secondary, &fixture_batch).await?;
    let before = store.load(&secondary).await?;
    let journal_messages = fixture.messages(store.config().stream_name()).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    let result = executor
        .execute(
            &TwoParticipants,
            CommandExecutionMetadata::new(
                fixture_batch.operation_id().clone(),
                ContentFingerprint::digest(b"reuse"),
            )
            .with_bounded_context(fixture.context.name().clone()),
            &Act {
                target: "a-primary".to_owned(),
                action: "create".to_owned(),
                item: String::new(),
            },
        )
        .await;
    assert!(
        matches!(result, Err(CommandExecutionError::Store(ref error)) if error.kind() == EventStoreErrorKind::IdentityConflict),
        "{result:?}"
    );
    assert!(store.load(&primary).await?.is_empty());
    assert_eq!(store.load(&secondary).await?, before);
    assert_eq!(
        fixture.messages(store.config().stream_name()).await?,
        journal_messages
    );
    assert!(matches!(
        store
            .lookup_outcome(&OutcomeKey::new(
                store.authority().clone(),
                fixture.context.name().clone(),
                fixture_batch.operation_id().clone(),
            ))
            .await?,
        OutcomeLookup::Missing
    ));
    let report = reports
        .recent()
        .into_iter()
        .find(|report| {
            report.operation_id.as_deref() == Some("fixture-F") && report.snapshot_version == 2
        })
        .ok_or("secondary snapshot report")?;
    assert_eq!((report.events_read, report.events_applied), (2, 0));
    Ok(())
}

#[tokio::test]
async fn history_only_snapshots_reconstruct_without_live_results_and_fence_recreated_journals()
-> TestResult {
    let (fixture, store, codecs, reports) =
        setup("snapshot-history-only", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 2).await?;
    let live = CommandExecutor::with_codecs(store.clone(), codecs.clone());
    live.create_snapshot::<Target>(&id, StreamVersion::new(2))
        .await?;
    live.execute(
        &Handler(Arc::new(AtomicUsize::new(0))),
        CommandExecutionMetadata::new(
            OperationId::new("completed-tail")?,
            ContentFingerprint::digest(b"completed-tail"),
        )
        .with_bounded_context(fixture.context.name().clone()),
        &Act {
            target: "one".to_owned(),
            action: "add".to_owned(),
            item: "tail".to_owned(),
        },
    )
    .await?;
    assert!(
        fixture
            .messages(store.config().outcome_state_stream_name())
            .await?
            > 0
    );
    fixture
        .js
        .delete_stream(store.config().outcome_state_stream_name())
        .await?;
    let history = NatsEventStore::connect_history(fixture.js.clone(), store.config().clone())
        .await?
        .with_snapshotting(true);
    let executor = CommandExecutor::with_codecs(history.clone(), codecs);
    assert_eq!(
        executor.rehydrate::<Target>(&id).await?.state().items.len(),
        3
    );
    let report = reports.recent().pop().ok_or("report")?;
    assert_eq!(
        (
            report.snapshot_version,
            report.events_read,
            report.events_applied
        ),
        (2, 1, 1)
    );
    assert_eq!(
        executor
            .rehydrate_at::<Target>(&id, StreamVersion::new(1))
            .await?
            .state()
            .items
            .len(),
        1
    );
    assert!(store.begin_command().await.is_err());
    assert!(history.begin_command().await.is_err());
    let journal = fixture.js.get_stream(store.config().stream_name()).await?;
    fixture
        .js
        .delete_stream(store.config().stream_name())
        .await?;
    fixture
        .js
        .create_stream(journal.cached_info().config.clone())
        .await?;
    assert!(matches!(executor.rehydrate::<Target>(&id).await,
        Err(SimulationError::Store(error)) if error.kind() == EventStoreErrorKind::ConfigurationMismatch));
    Ok(())
}

#[tokio::test]
async fn recapturing_an_unusable_checkpoint_persists_the_repaired_locator() -> TestResult {
    let (fixture, store, codecs, reports) =
        setup("snapshot-repair-locator", SnapshotPolicy::Manual).await?;
    let id = stream("one")?;
    seed(&store, &id, 8).await?;
    let scratch = codecs
        .as_ref()
        .clone()
        .with_snapshot_store(Arc::new(InMemorySnapshotStore::default()));
    let original = CommandExecutor::with_codecs(store.clone(), Arc::new(scratch))
        .create_snapshot::<Target>(&id, StreamVersion::new(6))
        .await?;
    let mut checkpoint = original.checkpoint.clone();
    let mut position: serde_json::Value = serde_json::from_slice(&checkpoint.position)?;
    position["sequence"] = serde_json::json!(999_999);
    checkpoint.position = serde_json::to_vec(&position)?;
    let broken = StoredSnapshot::new(
        original.replay_revision.clone(),
        original.schema_version,
        checkpoint,
        original.state.clone(),
    )?;
    let snapshots = NatsSnapshotStore::connect(
        fixture.js.clone(),
        NatsReadModelConfig::new(&fixture.context, "snapshots")?,
    )
    .await?;
    let key = SnapshotKey::new(store.authority().as_str(), &id, "v1", 1);
    snapshots.save(&key, &broken).await?;
    let executor = CommandExecutor::with_codecs(store.clone(), codecs);
    let (repaired, concurrent) = tokio::join!(
        executor.create_snapshot::<Target>(&id, StreamVersion::new(6)),
        executor.create_snapshot::<Target>(&id, StreamVersion::new(6)),
    );
    let repaired = repaired?;
    assert_eq!(concurrent?, repaired);
    assert_eq!(repaired.checkpoint, original.checkpoint);
    let persisted = snapshots
        .load(&key, StreamVersion::new(6))
        .await?
        .ok_or("missing persisted snapshot")?;
    let persisted_position: serde_json::Value =
        serde_json::from_slice(&persisted.checkpoint.position)?;
    let repaired_position: serde_json::Value =
        serde_json::from_slice(&repaired.checkpoint.position)?;
    assert_eq!(
        persisted_position["sequence"], repaired_position["sequence"],
        "successful capture must persist the repaired native locator"
    );
    assert_eq!(
        executor.rehydrate::<Target>(&id).await?.state().items.len(),
        8
    );
    let report = reports.recent().pop().ok_or("missing repaired report")?;
    assert_eq!(report.snapshot_status, SnapshotStatus::Hit);
    assert_eq!(report.snapshot_warning, None);
    assert_eq!(
        (
            report.snapshot_version,
            report.events_read,
            report.events_applied
        ),
        (6, 2, 2)
    );
    assert_eq!(store.audit_history(&id).await?.len(), 8);
    Ok(())
}
