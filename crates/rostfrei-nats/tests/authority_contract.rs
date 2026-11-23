//! Authority fencing at command admission, captured sessions and maintenance.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
#[allow(
    clippy::indexing_slicing,
    reason = "owned NATS wire-frame fault fixture"
)]
mod modular_state_fault_support;
mod release_contract_support;
mod transaction_support;
use async_trait::async_trait;
use modular_state_fault_support::StateProxy;
use release_contract_support::*;
use rostfrei_runtime::{
    CommandAccess, CommandProcessingService, CommandProcessorErrorKind, PrincipalId,
    TrustedCommandContext,
};
use transaction_support::AppendOneTransaction;
rostfrei_runtime::install_macro_support!();

async fn fixture(label: &str) -> TestResult<(Fixture, Arc<NatsEventStore>)> {
    let f = Fixture::new(label).await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_outcomes()?
        .with_explicit_detail_expiry(true)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?;
    provision_event_store(&f.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
    Ok((f, store))
}

fn proposal(f: &Fixture, operation: &str) -> TestResult<EventTransaction> {
    let operation = OperationId::new(operation)?;
    let fingerprint = ContentFingerprint::digest(operation.as_str());
    let evidence = CommandExecutionMetadata::new(operation.clone(), fingerprint)
        .with_default_outcome_evidence::<()>()
        .outcome_evidence()
        .cloned();
    Ok(EventTransaction::new(operation, fingerprint, vec![])
        .with_bounded_context(f.context.name().clone())
        .with_completion(CommandCompletion::new(evidence, None)))
}

fn body_config(f: &Fixture) -> TestResult<NatsEventStoreConfig> {
    Ok(NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_outcomes()?
        .with_storage_limits(512 * 1024, 8 * 1024)?
        .with_evicting_result_bodies(16 * 1024)?)
}

async fn roll_results(f: &Fixture, store: &NatsEventStore) -> TestResult {
    let mut bodies =
        f.js.get_stream(store.config().outcome_state_stream_name())
            .await?;
    let last = bodies.cached_info().state.last_sequence;
    assert!(last > 0);
    for i in 0..32 {
        store
            .begin_command()
            .await?
            .commit_outcome(proposal(f, &format!("pressure-{i}"))?)
            .await?;
        if bodies.info().await?.state.first_sequence > last {
            return Ok(());
        }
    }
    Err("result stream did not roll past the original bodies".into())
}

#[tokio::test]
async fn result_pressure_retains_every_decision_without_reevaluation() -> TestResult {
    use async_nats::jetstream::stream::DiscardPolicy;
    let f = Fixture::new("result-pressure").await?;
    let config = body_config(&f)?;
    provision_event_store(&f.js, &config).await?;
    let journal = f.js.get_stream(config.stream_name()).await?;
    let bodies = f.js.get_stream(config.outcome_state_stream_name()).await?;
    assert_eq!(journal.cached_info().config.discard, DiscardPolicy::New);
    assert_eq!(bodies.cached_info().config.discard, DiscardPolicy::Old);
    assert_eq!(bodies.cached_info().config.max_bytes, 16 * 1024);
    let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let service = processor(&f.context, store.clone(), calls.clone(), None)?;
    let bus = local_bus(&f.context, service.clone());
    let mut originals = Vec::new();
    for (action, target, decision) in [
        ("create", "one", OutcomeDecision::Accepted),
        ("add", "missing", OutcomeDecision::Rejected),
        ("noop", "unused", OutcomeDecision::Accepted),
    ] {
        let command = bus.encode(request(action, target, action)?)?;
        service.process(&command).await?;
        originals.push((command, decision));
    }
    roll_results(&f, &store).await?;
    for (command, decision) in originals {
        let key = OutcomeKey::new(
            store.authority().clone(),
            f.context.name().clone(),
            OperationId::new(command.operation_id().as_str())?,
        );
        let OutcomeLookup::Completed(winner) = store.lookup_outcome(&key).await? else {
            return Err("lost decision".into());
        };
        assert_eq!(winner.marker().decision(), decision);
        assert_eq!(winner.details(), &OutcomeDetails::NotRetained);
        assert_eq!(
            service.process(&command).await.unwrap_err().kind(),
            CommandProcessorErrorKind::CompletedDetailsNotRetained
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(store.load(&stream("one")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn acknowledged_prepare_evicted_before_commit_returns_the_journal_winner() -> TestResult {
    let f = Fixture::new("prepare-evicted").await?;
    let config = body_config(&f)?.with_puback_timeout(WAIT)?;
    provision_event_store(&f.js, &config).await?;
    let store = NatsEventStore::connect(f.js.clone(), config.clone()).await?;
    let proxy =
        StateProxy::start_with_timeout(config.outcome_state_stream_name(), "unused", 0, WAIT)
            .await?;
    let mut gate =
        proxy.gate_request(config.transaction_receipt_subject("gated", None, None), 1)?;
    let writer = NatsEventStore::connect(proxy.js.clone(), config).await?;
    let transaction = proposal(&f, "gated")?;
    let writing = tokio::spawn(async move {
        writer
            .begin_command()
            .await?
            .commit_outcome(transaction)
            .await
    });
    gate.wait().await?;
    roll_results(&f, &store).await?;
    gate.release()?;
    let committed = writing.await??;
    assert!(!committed.is_exact_replay());
    assert_eq!(
        committed.outcome().marker().decision(),
        OutcomeDecision::Accepted
    );
    assert_eq!(committed.outcome().details(), &OutcomeDetails::NotRetained);
    assert_eq!(
        store
            .lookup_outcome(committed.outcome().marker().detail_reference().key())
            .await?,
        OutcomeLookup::Completed(committed.outcome().clone())
    );
    Ok(())
}

async fn snapshot(f: &Fixture, config: &NatsEventStoreConfig) -> TestResult<serde_json::Value> {
    let mut streams = Vec::new();
    for name in [config.stream_name(), config.outcome_state_stream_name()] {
        let stream = f.js.get_stream(name).await?;
        let info = stream.cached_info();
        streams.push(serde_json::json!({
            "created": info.created.to_string(), "config": info.config,
            "messages": info.state.messages, "last": info.state.last_sequence,
        }));
    }
    Ok(serde_json::json!(streams))
}

async fn replace_pair(f: &Fixture, store: &NatsEventStore) -> TestResult {
    f.js.delete_stream(store.config().stream_name()).await?;
    f.js.delete_stream(store.config().outcome_state_stream_name())
        .await?;
    provision_event_store(&f.js, store.config()).await?;
    let replacement = NatsEventStore::connect(f.js.clone(), store.config().clone()).await?;
    assert_ne!(store.authority(), replacement.authority());
    Ok(())
}

#[tokio::test]
async fn captured_command_session_cannot_commit_to_recreated_authority() -> TestResult {
    let (f, store) = fixture("captured-command").await?;
    let session = store.begin_command().await?;
    replace_pair(&f, &store).await?;
    let before = snapshot(&f, store.config()).await?;
    assert_eq!(
        session
            .commit_outcome(proposal(&f, "stale")?)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch,
    );
    assert_eq!(snapshot(&f, store.config()).await?, before);
    Ok(())
}

#[tokio::test]
async fn cached_append_session_cannot_write_a_recreated_journal() -> TestResult {
    let (f, store) = fixture("captured-append").await?;
    let session = store.append_session().await?;
    let id = stream("cached")?;
    assert!(session.load(&id).await?.is_empty());
    replace_pair(&f, &store).await?;
    let before = snapshot(&f, store.config()).await?;
    let operation = OperationId::new("stale-append")?;
    let commit = derive_commit_id(&id, &operation);
    let batch = EventBatch::new(
        commit.clone(),
        operation,
        ContentFingerprint::digest("event"),
        vec![NewEvent::new(
            derive_event_id(&commit, 0),
            "changed",
            1,
            b"{}".to_vec(),
        )?],
    )?;
    assert_eq!(
        session
            .append_one_transaction(&id, ExpectedVersion::NoStream, batch)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch,
    );
    assert_eq!(snapshot(&f, store.config()).await?, before);
    Ok(())
}

#[derive(Default)]
struct AccessLog(std::sync::Mutex<Vec<CommandAccess>>);
#[async_trait]
impl rostfrei_runtime::CommandAuthorizer for AccessLog {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &rostfrei_runtime::ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), rostfrei_runtime::AuthorizationError> {
        self.0.lock().unwrap().push(access);
        Ok(())
    }
}

#[tokio::test]
async fn legacy_alias_cannot_append_to_label_free_authority() -> TestResult {
    let (f, store) = fixture("legacy-alias").await?;
    let alias_config = NatsEventStoreConfig::new(&f.context, store.config().stream_name())?
        .with_storage_limits(
            store.config().max_stream_bytes(),
            store.config().max_event_bytes(),
        )?;
    let alias = NatsEventStore::connect(f.js.clone(), alias_config).await?;
    f.js.delete_stream(store.config().outcome_state_stream_name())
        .await?;
    let transaction = EventTransaction::new(
        OperationId::new("legacy-alias-append")?,
        ContentFingerprint::digest("legacy-alias-append"),
        Vec::new(),
    )
    .with_bounded_context(f.context.name().clone());
    assert_eq!(
        alias
            .append_transaction(transaction)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert_eq!(f.messages(store.config().stream_name()).await?, 0);
    let mut journal =
        f.js.get_stream(store.config().stream_name())
            .await?
            .cached_info()
            .config
            .clone();
    journal
        .metadata
        .retain(|key, _| !key.starts_with("rostfrei."));
    f.js.update_stream(journal).await?;
    let transaction = EventTransaction::new(
        OperationId::new("cleared-alias-append")?,
        ContentFingerprint::digest("cleared-alias-append"),
        Vec::new(),
    )
    .with_bounded_context(f.context.name().clone());
    assert_eq!(
        alias
            .append_transaction(transaction)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert_eq!(f.messages(store.config().stream_name()).await?, 0);
    Ok(())
}

#[tokio::test]
async fn legacy_policy_update_cannot_strip_partial_authority_metadata() -> TestResult {
    let (f, store) = fixture("legacy-policy-alias").await?;
    let alias_config = NatsEventStoreConfig::new(&f.context, store.config().stream_name())?
        .with_storage_limits(
            store.config().max_stream_bytes(),
            store.config().max_event_bytes(),
        )?;
    let mut journal =
        f.js.get_stream(store.config().stream_name())
            .await?
            .cached_info()
            .config
            .clone();
    journal
        .metadata
        .remove("rostfrei.completed-outcomes.version");
    f.js.update_stream(journal).await?;
    let before = snapshot(&f, store.config()).await?;
    assert_eq!(
        rostfrei_nats::update_event_store(&f.js, &alias_config)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert_eq!(snapshot(&f, store.config()).await?, before);
    Ok(())
}

#[tokio::test]
async fn missing_result_authority_blocks_compatibility_append() -> TestResult {
    let (f, store) = fixture("missing-results-append").await?;
    let session = store.append_session().await?;
    f.js.delete_stream(store.config().outcome_state_stream_name())
        .await?;
    let operation = OperationId::new("unbound-append")?;
    let transaction = EventTransaction::new(
        operation,
        ContentFingerprint::digest("unbound-append"),
        Vec::new(),
    )
    .with_bounded_context(f.context.name().clone());
    assert_eq!(
        session
            .append_transaction(transaction)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert_eq!(f.messages(store.config().stream_name()).await?, 0);
    Ok(())
}

#[tokio::test]
async fn missing_state_is_not_authoritative_absence_or_permission_to_execute() -> TestResult {
    let (f, store) = fixture("missing-authority").await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(AccessLog::default());
    let mut service = CommandProcessor::builder(f.context.clone(), store.clone())
        .authorizer(log.clone())
        .build()?;
    service.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone())
        .encode(request("absent", "target", "create")?)?;
    let caller = TrustedCommandContext::new(
        PrincipalId::new("verified")?,
        SecurityScope::new("authority")?,
    );
    assert!(matches!(
        service.lookup(&caller, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    log.0.lock().unwrap().clear();
    f.js.delete_stream(store.config().outcome_state_stream_name())
        .await?;
    assert!(
        CommandProcessingService::process(&service, &caller, &command)
            .await
            .into_result()
            .is_err()
    );
    assert_eq!(*log.0.lock().unwrap(), vec![CommandAccess::ReadOutcome]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.messages(store.config().stream_name()).await?, 0);
    Ok(())
}

#[tokio::test]
async fn read_only_handle_replays_but_cannot_begin_new_work() -> TestResult {
    let (f, writer) = fixture("readonly-authority").await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let original = processor(&f.context, writer.clone(), calls.clone(), None)?;
    let command =
        local_bus(&f.context, original.clone()).encode(request("retained", "target", "create")?)?;
    let response = original.process(&command).await?;
    let readonly = Arc::new(
        NatsEventStore::connect(
            f.js.clone(),
            writer.config().clone().with_read_only_outcomes(),
        )
        .await?,
    );
    let before = snapshot(&f, writer.config()).await?;
    let reader = processor(&f.context, readonly.clone(), calls.clone(), None)?;
    assert_eq!(reader.process(&command).await?, response);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        readonly.load(&stream("target")?).await?,
        writer.load(&stream("target")?).await?
    );
    assert!(readonly.begin_command().await.is_err());
    assert!(readonly.append_session().await.is_err());
    assert_eq!(snapshot(&f, writer.config()).await?, before);
    Ok(())
}

fn assert_state_info_read(proxy: &StateProxy, config: &NatsEventStoreConfig, blocked: bool) {
    let requests = proxy.finish_counting();
    let subject = format!("$JS.API.STREAM.INFO.{}", config.outcome_state_stream_name());
    assert_eq!(requests.get(&subject).copied(), Some(1), "{requests:?}");
    assert_eq!(
        proxy.faults.dropped_reads.swap(0, Ordering::SeqCst),
        usize::from(blocked),
        "{requests:?}"
    );
}

#[tokio::test]
async fn state_info_outage_blocks_execution_and_recovers_without_a_decision() -> TestResult {
    let (f, original) = fixture("state-info-outage").await?;
    // A healthy journal read must reach the faulted state INFO, not time out first.
    let proxy = StateProxy::start_with_timeout(
        original.config().outcome_state_stream_name(),
        "unused",
        0,
        WAIT,
    )
    .await?;
    let store =
        Arc::new(NatsEventStore::connect(proxy.js.clone(), original.config().clone()).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(AccessLog::default());
    let mut service = CommandProcessor::builder(f.context.clone(), store.clone())
        .authorizer(log.clone())
        .build()?;
    service.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone())
        .encode(request("absent", "target", "create")?)?;
    let caller = TrustedCommandContext::new(
        PrincipalId::new("verified")?,
        SecurityScope::new("authority")?,
    );
    let key = OutcomeKey::new(
        store.authority().clone(),
        f.context.name().clone(),
        OperationId::new("absent")?,
    );
    proxy.start_counting();
    assert!(matches!(
        service.lookup(&caller, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    assert_state_info_read(&proxy, store.config(), false);
    log.0.lock().unwrap().clear();
    let before = snapshot(&f, store.config()).await?;
    proxy.faults.block_state_info.store(true, Ordering::SeqCst);

    proxy.start_counting();
    let error = store.lookup_outcome(&key).await.unwrap_err();
    assert_eq!(error.kind(), EventStoreErrorKind::Unavailable, "{error:?}");
    assert_state_info_read(&proxy, store.config(), true);
    proxy.start_counting();
    let error = service
        .lookup(&caller, &command)
        .await
        .into_result()
        .unwrap_err();
    assert_eq!(
        error.kind(),
        CommandProcessorErrorKind::Unavailable,
        "{error:?}"
    );
    assert_state_info_read(&proxy, store.config(), true);
    proxy.start_counting();
    let error = CommandProcessingService::process(&service, &caller, &command)
        .await
        .into_result()
        .unwrap_err();
    assert_eq!(
        error.kind(),
        CommandProcessorErrorKind::Unavailable,
        "{error:?}"
    );
    assert_state_info_read(&proxy, store.config(), true);
    assert_eq!(
        *log.0.lock().unwrap(),
        [CommandAccess::ReadOutcome, CommandAccess::ReadOutcome]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot(&f, store.config()).await?, before);

    proxy.faults.block_state_info.store(false, Ordering::SeqCst);
    proxy.start_counting();
    assert!(matches!(
        store.lookup_outcome(&key).await?,
        OutcomeLookup::Missing
    ));
    assert_state_info_read(&proxy, store.config(), false);
    Ok(())
}

#[tokio::test]
async fn stale_expiry_cannot_mutate_either_replacement_stream() -> TestResult {
    let (f, store) = fixture("stale-expiry").await?;
    let winner = store
        .begin_command()
        .await?
        .commit_outcome(proposal(&f, "winner")?)
        .await?;
    let marker = winner.outcome().marker();
    replace_pair(&f, &store).await?;
    let before = snapshot(&f, store.config()).await?;
    assert!(matches!(
        store.expire_details(marker.detail_reference().key(), marker.revision()).await,
        Err(OutcomeLifecycleError::Store(ref e)) if e.kind() == EventStoreErrorKind::ConfigurationMismatch
    ));
    assert_eq!(snapshot(&f, store.config()).await?, before);
    Ok(())
}
