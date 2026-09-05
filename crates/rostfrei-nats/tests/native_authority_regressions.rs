//! N1/N2/Q1 route isolation: real authority loss, replay-only handles and eligible absence.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
#[allow(
    clippy::indexing_slicing,
    reason = "existing relay parses frames from the owned broker"
)]
mod modular_state_fault_support;
mod release_contract_support;
use async_trait::async_trait;
use modular_state_fault_support::StateProxy;
use release_contract_support::*;
use rostfrei::{CommandAccess, CommandProcessingService, PrincipalId, TrustedCommandContext};
rostfrei::install_macro_support!();

static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

// This matrix tests authority, not capacity. Bound each disposable namespace so
// all independent routes can coexist in the authenticated runner's storage budget.
async fn fixture(label: &str) -> TestResult<Fixture> {
    let application = ApplicationName::new(format!(
        "{label}-{}-{}",
        std::process::id(),
        FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
    ))?;
    let context = application.bounded_context("release-contract")?;
    let messaging =
        ApplicationMessagingConfig::new(&application)?.with_max_bytes(4 * 1024 * 1024)?;
    let connection = connect(&NatsConnectionConfig::new(
        "authority-regressions",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let js = connection.jetstream().clone();
    provision_application_messaging(&js, &messaging).await?;
    let adapter = Arc::new(NatsMessagingAdapter::new(
        NatsPublisher::new(js.clone(), messaging.topology().clone()),
        NatsCommandResponseReader::new(js.clone(), messaging.topology().clone()),
    ));
    Ok(Fixture {
        context,
        js,
        messaging,
        adapter,
    })
}
fn bounded_config(f: &Fixture) -> TestResult<NatsEventStoreConfig> {
    Ok(NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?)
}
async fn split_store(f: &Fixture) -> TestResult<Arc<NatsEventStore>> {
    let config = bounded_config(f)?.with_fresh_split_outcome_epoch("authority")?;
    provision_event_store(&f.js, &config).await?;
    Ok(Arc::new(
        NatsEventStore::connect(f.js.clone(), config).await?,
    ))
}

fn caller() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("authority-regression")?,
        SecurityScope::new("authority-scope")?,
    ))
}
fn transaction(f: &Fixture, operation: &str) -> TestResult<EventTransaction> {
    Ok(EventTransaction::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(operation),
        vec![],
    )
    .with_bounded_context(f.context.name().clone()))
}
fn completed(f: &Fixture, operation: &str) -> TestResult<EventTransaction> {
    let p = transaction(f, operation)?;
    let evidence =
        CommandExecutionMetadata::new(p.operation_id().clone(), p.operation_fingerprint())
            .with_default_outcome_evidence::<()>()
            .outcome_evidence()
            .cloned();
    Ok(p.with_completion(CommandCompletion::new(evidence, None)))
}
fn batch(id: &StreamId, operation: &str) -> TestResult<EventBatch> {
    let operation = OperationId::new(operation)?;
    let commit = derive_commit_id(id, &operation);
    Ok(EventBatch::new(
        commit.clone(),
        operation,
        ContentFingerprint::digest("authority-event"),
        vec![NewEvent::new(
            derive_event_id(&commit, 0),
            "changed",
            1,
            serde_json::to_vec(&Changed {
                created: true,
                item: None,
            })?,
        )?],
    )?)
}
fn rejection(f: &Fixture) -> TestResult<(rostfrei::EncodedCommand, CommandResponse)> {
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "original-rejection",
        "missing",
        "add",
    )?)?;
    let response = CommandResponse::rejected(
        rostfrei::command_response_message_id(command.message_id())?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
        map_rejection(&"target-not-found")?,
    )?;
    Ok((command, response))
}
async fn snapshot(f: &Fixture, config: &NatsEventStoreConfig) -> TestResult<serde_json::Value> {
    let mut streams = Vec::new();
    for name in [config.stream_name(), config.outcome_state_stream_name()]
        .into_iter()
        .filter(|name| !name.is_empty())
    {
        match f.js.get_stream(name).await {
            Ok(s) => streams.push(serde_json::json!({
                "name": name, "created": s.cached_info().created.to_string(),
                "config": s.cached_info().config,
                "messages": s.cached_info().state.messages,
                "last": s.cached_info().state.last_sequence,
            })),
            Err(e) => {
                assert!(
                    matches!(e.kind(), async_nats::jetstream::context::GetStreamErrorKind::JetStream(ref e) if e.code() == 404)
                );
                streams.push(serde_json::json!({"missing": name}));
            }
        }
    }
    Ok(serde_json::json!(streams))
}

#[derive(Clone, Copy, Debug)]
enum Damage {
    UnmarkedJournal,
    MissingJournal,
    MissingState,
    RecreatedState,
    RecreatedPair,
    StateConfig,
}
const DAMAGES: [Damage; 6] = [
    Damage::UnmarkedJournal,
    Damage::MissingJournal,
    Damage::MissingState,
    Damage::RecreatedState,
    Damage::RecreatedPair,
    Damage::StateConfig,
];
async fn damage(f: &Fixture, config: &NatsEventStoreConfig, fault: Damage) -> TestResult {
    match fault {
        Damage::UnmarkedJournal => {
            let original = f.js.get_stream(config.stream_name()).await?;
            let mut replacement = original.cached_info().config.clone();
            replacement.metadata.clear();
            f.js.delete_stream(config.stream_name()).await?;
            let current = f.js.create_stream(replacement).await?;
            assert_ne!(
                original.cached_info().created,
                current.cached_info().created
            );
        }
        Damage::MissingJournal => {
            f.js.delete_stream(config.stream_name()).await?;
        }
        Damage::MissingState => {
            f.js.delete_stream(config.outcome_state_stream_name())
                .await?;
        }
        Damage::RecreatedState => {
            let old = f.js.get_stream(config.outcome_state_stream_name()).await?;
            f.js.delete_stream(config.outcome_state_stream_name())
                .await?;
            let current = f.js.create_stream(old.cached_info().config.clone()).await?;
            assert_ne!(old.cached_info().created, current.cached_info().created);
        }
        Damage::RecreatedPair => {
            let old = NatsEventStore::connect(f.js.clone(), config.clone()).await?;
            f.js.delete_stream(config.stream_name()).await?;
            f.js.delete_stream(config.outcome_state_stream_name())
                .await?;
            provision_event_store(&f.js, config).await?;
            let current = NatsEventStore::connect(f.js.clone(), config.clone()).await?;
            assert_ne!(old.authority(), current.authority());
        }
        Damage::StateConfig => {
            let mut state =
                f.js.get_stream(config.outcome_state_stream_name())
                    .await?
                    .cached_info()
                    .config
                    .clone();
            state.allow_direct = true;
            f.js.update_stream(state).await?;
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug)]
enum Route {
    Append,
    Transaction,
    NewAppend,
    NewTransaction,
    OpenAppend,
    OpenTransaction,
    Completed,
    NewCompleted,
    OpenCompleted,
    CommandCommit,
    Import,
    NewLoad,
    OpenLoad,
}

#[allow(
    clippy::too_many_lines,
    reason = "one isolated mutation route across each authority-loss fixture"
)]
async fn stale_route(route: Route) -> TestResult {
    for fault in DAMAGES {
        let f = fixture("authority-stale-route").await?;
        let store = split_store(&f).await?;
        let open = store.append_session().await?;
        let command_session = store.begin_command().await?;
        let id = stream("mutation")?;
        open.load(&id).await?; // Cache a genuine old-incarnation history.
        damage(&f, store.config(), fault).await?;
        let before = snapshot(&f, store.config()).await?;
        let result = match route {
            Route::Append => store
                .append(&id, ExpectedVersion::NoStream, batch(&id, "direct")?)
                .await
                .map(|_| ()),
            Route::Transaction => store
                .append_transaction(transaction(&f, "direct")?)
                .await
                .map(|_| ()),
            Route::NewAppend => store
                .append_session()
                .await?
                .append(&id, ExpectedVersion::NoStream, batch(&id, "new")?)
                .await
                .map(|_| ()),
            Route::NewTransaction => store
                .append_session()
                .await?
                .append_transaction(transaction(&f, "new")?)
                .await
                .map(|_| ()),
            Route::OpenAppend => open
                .append(&id, ExpectedVersion::NoStream, batch(&id, "open")?)
                .await
                .map(|_| ()),
            Route::OpenTransaction => open
                .append_transaction(transaction(&f, "open")?)
                .await
                .map(|_| ()),
            Route::Completed => store
                .commit_outcome(completed(&f, "direct")?)
                .await
                .map(|_| ()),
            Route::NewCompleted => store
                .append_session()
                .await?
                .commit_outcome(completed(&f, "new")?)
                .await
                .map(|_| ()),
            Route::OpenCompleted => open
                .commit_outcome(completed(&f, "open")?)
                .await
                .map(|_| ()),
            Route::CommandCommit => command_session
                .commit_outcome(completed(&f, "command")?)
                .await
                .map(|_| ()),
            Route::Import => {
                let (command, response) = rejection(&f)?;
                store
                    .import_legacy_rejected_outcome(&command, &response, f.messaging.topology())
                    .await
                    .map(|_| ())
            }
            Route::NewLoad | Route::OpenLoad => {
                // Session history only promises incarnation fencing; state-only faults
                // are checked at mutation/admission and don't block ordinary history.
                if !matches!(
                    fault,
                    Damage::UnmarkedJournal | Damage::MissingJournal | Damage::RecreatedPair
                ) {
                    continue;
                }
                if matches!(route, Route::NewLoad) {
                    store.append_session().await?.load(&id).await.map(|_| ())
                } else {
                    open.load(&id).await.map(|_| ())
                }
            }
        };
        let error = result.unwrap_err();
        // Legacy session incarnation inspection classifies a missing journal as unavailable.
        let expected = if matches!(fault, Damage::MissingJournal)
            && matches!(
                route,
                Route::NewCompleted
                    | Route::OpenCompleted
                    | Route::CommandCommit
                    | Route::NewLoad
                    | Route::OpenLoad
            ) {
            EventStoreErrorKind::Unavailable
        } else {
            EventStoreErrorKind::ConfigurationMismatch
        };
        assert_eq!(error.kind(), expected, "{route:?}/{fault:?}: {error:?}");
        assert_eq!(
            snapshot(&f, store.config()).await?,
            before,
            "{route:?}/{fault:?}"
        );
    }
    Ok(())
}
macro_rules! stale_cases {
    ($($name:ident => $route:ident),+ $(,)?) => { $(
        #[tokio::test]
        async fn $name() -> TestResult { stale_route(Route::$route).await }
    )+ };
}
stale_cases! {
    stale_direct_append => Append,
    stale_direct_transaction => Transaction,
    stale_new_session_append => NewAppend,
    stale_new_session_transaction => NewTransaction,
    stale_open_session_append => OpenAppend,
    stale_open_session_transaction => OpenTransaction,
    stale_direct_completed => Completed,
    stale_new_session_completed => NewCompleted,
    stale_open_session_completed => OpenCompleted,
    stale_open_command_commit => CommandCommit,
    stale_import => Import,
    new_session_never_adopts_replacement_journal => NewLoad,
    open_session_keeps_original_journal => OpenLoad,
}

#[tokio::test]
async fn stale_expiry_preserves_both_replacement_streams() -> TestResult {
    for fault in DAMAGES {
        let f = fixture("authority-stale-expiry").await?;
        let config = bounded_config(&f)?
            .with_fresh_split_outcome_epoch("expiry")?
            .with_explicit_detail_expiry(true)?;
        provision_event_store(&f.js, &config).await?;
        let store = NatsEventStore::connect(f.js.clone(), config).await?;
        let winner = store
            .begin_command()
            .await?
            .commit_outcome(completed(&f, "winner")?)
            .await?;
        let marker = winner.outcome().marker();
        damage(&f, store.config(), fault).await?;
        let before = snapshot(&f, store.config()).await?;
        assert!(
            matches!(store.expire_details(marker.detail_reference().key(), marker.revision()).await,
            Err(OutcomeLifecycleError::Store(ref e)) if e.kind() == EventStoreErrorKind::ConfigurationMismatch)
        );
        assert_eq!(snapshot(&f, store.config()).await?, before);
    }
    Ok(())
}

struct AccessLog(std::sync::Mutex<Vec<CommandAccess>>);
#[async_trait]
impl rostfrei::CommandAuthorizer for AccessLog {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &rostfrei::ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), rostfrei::AuthorizationError> {
        self.0.lock().unwrap().push(access);
        Ok(())
    }
}
fn service(
    f: &Fixture,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
    log: Arc<AccessLog>,
) -> TestResult<CommandProcessor> {
    let mut p = CommandProcessor::builder(f.context.clone(), store)
        .authorizer(log)
        .build()?;
    p.register::<Act, _>(Handler(calls), map_rejection)?;
    Ok(p)
}
async fn missing_refuses(fault: Option<Damage>) -> TestResult {
    let f = fixture("authority-missing").await?;
    let original = split_store(&f).await?;
    let proxy =
        StateProxy::start(original.config().outcome_state_stream_name(), "unused", 0).await?;
    let store =
        Arc::new(NatsEventStore::connect(proxy.js.clone(), original.config().clone()).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(AccessLog(std::sync::Mutex::new(Vec::new())));
    let p = service(&f, store.clone(), calls.clone(), log.clone())?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone())
        .encode(request("absent", "target", "create")?)?;
    let key = OutcomeKey::new(
        store.authority().clone(),
        f.context.name().clone(),
        OperationId::new("absent")?,
    );
    assert!(matches!(
        p.lookup(&caller()?, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    log.0.lock().unwrap().clear();
    if let Some(fault) = fault {
        damage(&f, store.config(), fault).await?;
    } else {
        proxy.faults.block_state_info.store(true, Ordering::SeqCst);
    }
    let before = snapshot(&f, store.config()).await?;
    let expected = if fault.is_some() {
        EventStoreErrorKind::ConfigurationMismatch
    } else {
        EventStoreErrorKind::Unavailable
    };
    assert_eq!(
        store.lookup_outcome(&key).await.unwrap_err().kind(),
        expected
    );
    assert!(p.lookup(&caller()?, &command).await.into_result().is_err());
    assert!(
        CommandProcessingService::process(&p, &caller()?, &command)
            .await
            .into_result()
            .is_err()
    );
    assert_eq!(
        *log.0.lock().unwrap(),
        vec![CommandAccess::ReadOutcome, CommandAccess::ReadOutcome]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot(&f, store.config()).await?, before);
    if fault.is_none() {
        assert!(proxy.faults.dropped_reads.load(Ordering::SeqCst) >= 3);
        proxy.faults.block_state_info.store(false, Ordering::SeqCst);
        assert!(matches!(
            store.lookup_outcome(&key).await?,
            OutcomeLookup::Missing
        ));
    }
    Ok(())
}
#[tokio::test]
async fn missing_state_prevents_execute_authorization() -> TestResult {
    missing_refuses(Some(Damage::MissingState)).await
}
#[tokio::test]
async fn recreated_state_prevents_execute_authorization() -> TestResult {
    missing_refuses(Some(Damage::RecreatedState)).await
}
#[tokio::test]
async fn misconfigured_state_prevents_execute_authorization() -> TestResult {
    missing_refuses(Some(Damage::StateConfig)).await
}
#[tokio::test]
async fn unavailable_state_prevents_execute_authorization() -> TestResult {
    missing_refuses(None).await
}

#[tokio::test]
async fn inline_and_unmarked_missing_are_not_permission_to_execute() -> TestResult {
    for inline in [false, true] {
        let f = fixture("authority-legacy-missing").await?;
        let config = bounded_config(&f)?;
        let config = if inline {
            config.with_fresh_completed_outcome_epoch("inline")?
        } else {
            config
        };
        provision_event_store(&f.js, &config).await?;
        let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
        let log = Arc::new(AccessLog(std::sync::Mutex::new(Vec::new())));
        let calls = Arc::new(AtomicUsize::new(0));
        let p = service(&f, store.clone(), calls.clone(), log.clone())?;
        let command = CommandBus::new(f.context.clone(), f.adapter.clone())
            .encode(request("missing", "target", "create")?)?;
        assert!(p.lookup(&caller()?, &command).await.into_result().is_err());
        assert!(
            CommandProcessingService::process(&p, &caller()?, &command)
                .await
                .into_result()
                .is_err()
        );
        assert_eq!(
            *log.0.lock().unwrap(),
            vec![CommandAccess::ReadOutcome, CommandAccess::ReadOutcome]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.messages(store.config().stream_name()).await?, 0);
    }
    Ok(())
}

struct ZeroBudget(Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<()> for ZeroBudget {
    type Rejection = String;
    async fn handle(&self, (): &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(CommandDecision::Accepted)
    }
}
#[tokio::test]
async fn healthy_missing_executes_and_replays_with_zero_retry_budget() -> TestResult {
    let f = fixture("authority-zero").await?;
    let store = split_store(&f).await?;
    let operation = OperationId::new("zero-budget")?;
    let key = OutcomeKey::new(
        store.authority().clone(),
        f.context.name().clone(),
        operation.clone(),
    );
    assert!(matches!(
        store.lookup_outcome(&key).await?,
        OutcomeLookup::Missing
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = CommandExecutor::new(store.clone()).with_max_conflict_retries(0);
    let metadata = CommandExecutionMetadata::new(operation, ContentFingerprint::digest("zero"))
        .with_bounded_context(f.context.name().clone());
    for _ in 0..2 {
        executor
            .execute_with_rejection_codec(
                &ZeroBudget(calls.clone()),
                metadata.clone(),
                &(),
                &JsonRejectionCodec::new("zero", 1),
            )
            .await?;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.messages(store.config().stream_name()).await?, 1);
    assert_eq!(
        f.messages(store.config().outcome_state_stream_name())
            .await?,
        1
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum ReadOnlyRoute {
    Append,
    Transaction,
    Session,
    Completed,
    Import,
    Expiry,
}
#[allow(
    clippy::too_many_lines,
    reason = "same mutation refusal and real-read controls across three supported storage profiles"
)]
async fn read_only_route(route: ReadOnlyRoute) -> TestResult {
    for profile in ["legacy", "inline", "split"] {
        let f = fixture("authority-readonly").await?;
        let base =
            bounded_config(&f)?.with_legacy_security_scope(caller()?.security_scope().clone());
        let config = match profile {
            "split" => base
                .with_fresh_split_outcome_epoch("readonly")?
                .with_explicit_detail_expiry(true)?,
            "inline" => base.with_fresh_completed_outcome_epoch("readonly")?,
            _ => base,
        };
        provision_event_store(&f.js, &config).await?;
        let writer = NatsEventStore::connect(f.js.clone(), config.clone()).await?;
        let id = stream("seed")?;
        writer
            .append(&id, ExpectedVersion::NoStream, batch(&id, "seed-event")?)
            .await?;
        writer
            .append_session()
            .await?
            .append_transaction(transaction(&f, "seed-receipt")?)
            .await?;
        let (command, response) = rejection(&f)?;
        f.publisher()
            .publish_command_response(OutboundMessage::json(
                command.response_address()?,
                response.message_id().clone(),
                &response,
            )?)
            .await?;
        if profile == "legacy" {
            writer
                .import_legacy_rejected_outcome(&command, &response, f.messaging.topology())
                .await?;
        } else {
            writer
                .commit_outcome(
                    command
                        .legacy_rejection_transaction(&response)?
                        .with_security_scope(caller()?.security_scope().clone()),
                )
                .await?;
        }
        let readonly = Arc::new(
            NatsEventStore::connect(f.js.clone(), config.with_read_only_outcomes()).await?,
        );
        let before = snapshot(&f, readonly.config()).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let log = Arc::new(AccessLog(std::sync::Mutex::new(Vec::new())));
        let p = service(&f, readonly.clone(), calls.clone(), log.clone())?;
        let replay = CommandProcessingService::process(&p, &caller()?, &command)
            .await
            .into_result()?;
        assert_eq!(replay.admit(&caller()?, &command)?, &response);
        assert_eq!(replay.origin(), rostfrei::CompletionOrigin::Replayed);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(*log.0.lock().unwrap(), vec![CommandAccess::ReadOutcome]);
        assert_eq!(readonly.load(&id).await?, writer.load(&id).await?);
        assert_eq!(
            readonly.audit_history(&id).await?,
            writer.audit_history(&id).await?
        );
        assert_eq!(
            readonly
                .list_streams(&AggregateType::new("target")?)
                .await?,
            writer.list_streams(&AggregateType::new("target")?).await?
        );
        assert!(
            readonly
                .load_transaction_receipt(&OperationId::new("seed-receipt")?)
                .await?
                .is_some()
        );
        assert_eq!(
            readonly
                .load_completed_outcome_in_context(
                    f.context.name(),
                    &OperationId::new(command.operation_id().as_str())?
                )
                .await?,
            writer
                .load_completed_outcome_in_context(
                    f.context.name(),
                    &OperationId::new(command.operation_id().as_str())?
                )
                .await?
        );
        let unknown = OutcomeKey::new(
            readonly.authority().clone(),
            f.context.name().clone(),
            OperationId::new("unknown")?,
        );
        assert_eq!(
            readonly.lookup_outcome(&unknown).await.unwrap_err().kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
        assert!(readonly.begin_command().await.is_err());
        let result = match route {
            ReadOnlyRoute::Append => readonly
                .append(
                    &stream("new")?,
                    ExpectedVersion::NoStream,
                    batch(&stream("new")?, "new-event")?,
                )
                .await
                .map(|_| ()),
            ReadOnlyRoute::Transaction => readonly
                .append_transaction(transaction(&f, "new-receipt")?)
                .await
                .map(|_| ()),
            ReadOnlyRoute::Session => readonly.append_session().await.map(|_| ()),
            ReadOnlyRoute::Completed => readonly
                .commit_outcome(completed(&f, "new-completion")?)
                .await
                .map(|_| ()),
            ReadOnlyRoute::Import => readonly
                .import_legacy_rejected_outcome(&command, &response, f.messaging.topology())
                .await
                .map(|_| ()),
            ReadOnlyRoute::Expiry => {
                assert_eq!(
                    readonly
                        .expire_details(
                            replay.outcome().marker().detail_reference().key(),
                            replay.outcome().marker().revision()
                        )
                        .await
                        .unwrap_err(),
                    OutcomeLifecycleError::Unsupported
                );
                assert_eq!(snapshot(&f, readonly.config()).await?, before);
                continue;
            }
        };
        assert_eq!(
            result.unwrap_err().kind(),
            EventStoreErrorKind::ConfigurationMismatch
        );
        assert_eq!(snapshot(&f, readonly.config()).await?, before);
    }
    Ok(())
}
macro_rules! readonly_cases {
    ($($name:ident => $route:ident),+ $(,)?) => { $(
        #[tokio::test]
        async fn $name() -> TestResult { read_only_route(ReadOnlyRoute::$route).await }
    )+ };
}
readonly_cases! {
    readonly_refuses_append_and_preserves_reads => Append,
    readonly_refuses_transaction_and_preserves_reads => Transaction,
    readonly_refuses_session_and_preserves_reads => Session,
    readonly_refuses_completed_write_and_preserves_reads => Completed,
    readonly_refuses_import_and_preserves_reads => Import,
    readonly_refuses_expiry_and_preserves_reads => Expiry,
}
