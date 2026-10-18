//! Original reviewer's adversarial repair confirmation on the frozen native candidate.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::too_many_lines
)]
#[allow(clippy::indexing_slicing, reason = "existing owned wire relay")]
mod modular_state_fault_support;
mod release_contract_support;
use async_trait::async_trait;
use modular_state_fault_support::StateProxy;
use release_contract_support::*;
use rostfrei::{CommandAccess, CommandProcessingService, PrincipalId, TrustedCommandContext};
rostfrei::install_macro_support!();

fn caller() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("independent-rereview")?,
        SecurityScope::new("rereview-scope")?,
    ))
}
fn transaction(f: &Fixture, op: &str) -> TestResult<EventTransaction> {
    Ok(EventTransaction::new(
        OperationId::new(op)?,
        ContentFingerprint::digest(op),
        vec![],
    )
    .with_bounded_context(f.context.name().clone()))
}
fn completed(f: &Fixture, op: &str) -> TestResult<EventTransaction> {
    let p = transaction(f, op)?;
    let evidence =
        CommandExecutionMetadata::new(p.operation_id().clone(), p.operation_fingerprint())
            .with_default_outcome_evidence::<()>()
            .outcome_evidence()
            .cloned();
    Ok(p.with_completion(CommandCompletion::new(evidence, None)))
}
fn batch(id: &StreamId, op: &str) -> TestResult<EventBatch> {
    let op = OperationId::new(op)?;
    let commit = derive_commit_id(id, &op);
    Ok(EventBatch::new(
        commit.clone(),
        op,
        ContentFingerprint::digest("event"),
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
async fn split(f: &Fixture) -> TestResult<Arc<NatsEventStore>> {
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?
        .with_fresh_split_outcome_epoch("independent-rereview")?
        .with_explicit_detail_expiry(true)?;
    provision_event_store(&f.js, &config).await?;
    Ok(Arc::new(
        NatsEventStore::connect(f.js.clone(), config).await?,
    ))
}
async fn snapshot(f: &Fixture, store: &NatsEventStore) -> TestResult<serde_json::Value> {
    let mut all = Vec::new();
    for name in [
        store.config().stream_name(),
        store.config().outcome_state_stream_name(),
    ]
    .into_iter()
    .filter(|n| !n.is_empty())
    {
        let s = f.js.get_stream(name).await?;
        all.push(
            serde_json::json!({"created": s.cached_info().created.to_string(),
            "config": s.cached_info().config, "state": format!("{:?}", s.cached_info().state)}),
        );
    }
    Ok(serde_json::json!(all))
}
fn configuration<T>(result: Result<T, EventStoreError>) {
    assert_eq!(
        result.err().unwrap().kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
}
#[derive(Default)]
struct AccessLog(std::sync::Mutex<Vec<CommandAccess>>);
#[async_trait]
impl rostfrei::CommandAuthorizer for AccessLog {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &rostfrei::ValidatedCommand,
        phase: CommandAccess,
    ) -> Result<(), rostfrei::AuthorizationError> {
        self.0.lock().unwrap().push(phase);
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

#[tokio::test]
async fn in_place_metadata_damage_blocks_even_exact_compatibility_replays_then_recovers()
-> TestResult {
    for fault in [
        "journal-epoch",
        "journal-binding",
        "state-epoch",
        "state-binding",
        "journal-policy",
    ] {
        let f = Fixture::new("rereview-in-place").await?;
        let store = split(&f).await?;
        let id = stream("retained-event")?;
        let event = batch(&id, "retained-event")?;
        store
            .append(&id, ExpectedVersion::NoStream, event.clone())
            .await?;
        let tx = transaction(&f, "retained-tx")?;
        store.append_transaction(tx.clone()).await?;
        let p = completed(&f, "winner")?;
        let winner = store
            .begin_command()
            .await?
            .commit_outcome(p.clone())
            .await?;
        let marker = winner.outcome().marker();
        let open_append = store.append_session().await?;
        open_append.load(&id).await?;
        let open_tx = store.append_session().await?;
        let open_completed = store.append_session().await?;
        let open_command = store.begin_command().await?;
        let name = if fault.starts_with("journal") {
            store.config().stream_name()
        } else {
            store.config().outcome_state_stream_name()
        };
        let original = f.js.get_stream(name).await?;
        let mut changed = original.cached_info().config.clone();
        match fault {
            "journal-epoch" | "state-epoch" => {
                changed.metadata.remove("rostfrei.completed-outcomes.epoch");
            }
            "journal-binding" => {
                changed.metadata.remove("rostfrei.split.state-incarnation");
            }
            "state-binding" => {
                changed
                    .metadata
                    .remove("rostfrei.split.journal-incarnation");
            }
            _ => {
                changed.allow_direct = true;
            }
        }
        f.js.update_stream(changed).await?;
        assert_eq!(
            f.js.get_stream(name).await?.cached_info().created,
            original.cached_info().created
        );
        let before = snapshot(&f, &store).await?;
        configuration(
            store
                .append(&id, ExpectedVersion::NoStream, event.clone())
                .await,
        );
        configuration(store.append_transaction(tx.clone()).await);
        configuration(
            open_append
                .append(&id, ExpectedVersion::NoStream, event.clone())
                .await,
        );
        configuration(open_tx.append_transaction(tx.clone()).await);
        configuration(
            store
                .append_session()
                .await?
                .append_transaction(tx.clone())
                .await,
        );
        configuration(
            store
                .append_session()
                .await?
                .append(&id, ExpectedVersion::NoStream, event.clone())
                .await,
        );
        configuration(store.commit_outcome(p.clone()).await);
        configuration(open_completed.commit_outcome(p.clone()).await);
        configuration(
            store
                .append_session()
                .await?
                .commit_outcome(p.clone())
                .await,
        );
        configuration(open_command.commit_outcome(p.clone()).await);
        configuration(store.begin_command().await);
        configuration(
            store
                .lookup_outcome(&OutcomeKey::new(
                    store.authority().clone(),
                    f.context.name().clone(),
                    OperationId::new("missing")?,
                ))
                .await,
        );
        configuration(store.lookup_outcome(marker.detail_reference().key()).await);
        assert!(
            matches!(store.expire_details(marker.detail_reference().key(), marker.revision()).await,
            Err(OutcomeLifecycleError::Store(ref e)) if e.kind() == EventStoreErrorKind::ConfigurationMismatch)
        );
        assert_eq!(snapshot(&f, &store).await?, before, "{fault}");
        f.js.update_stream(original.cached_info().config.clone())
            .await?;
        assert!(matches!(
            store.append(&id, ExpectedVersion::NoStream, event).await?,
            AppendOutcome::ExactReplay(_)
        ));
        assert!(matches!(
            store.append_transaction(tx).await?,
            TransactionAppendOutcome::ExactReplay(_)
        ));
        assert!(
            store
                .begin_command()
                .await?
                .commit_outcome(p)
                .await?
                .is_exact_replay()
        );
    }
    Ok(())
}

#[tokio::test]
async fn repairing_cross_bindings_cannot_hide_state_reincarnation_from_captured_handles()
-> TestResult {
    let f = Fixture::new("rereview-rebound-state").await?;
    let store = split(&f).await?;
    let open_append = store.append_session().await?;
    let open_tx = store.append_session().await?;
    let open_completed = store.append_session().await?;
    let open_command = store.begin_command().await?;
    let winner = store
        .begin_command()
        .await?
        .commit_outcome(completed(&f, "old-winner")?)
        .await?;
    let marker = winner.outcome().marker();
    let journal = f.js.get_stream(store.config().stream_name()).await?;
    let state =
        f.js.get_stream(store.config().outcome_state_stream_name())
            .await?;
    f.js.delete_stream(store.config().outcome_state_stream_name())
        .await?;
    let replacement =
        f.js.create_stream(state.cached_info().config.clone())
            .await?;
    assert_ne!(
        state.cached_info().created,
        replacement.cached_info().created
    );
    let mut rebound = journal.cached_info().config.clone();
    rebound.metadata.insert(
        "rostfrei.split.state-incarnation".to_owned(),
        replacement.cached_info().created.to_string(),
    );
    f.js.update_stream(rebound).await?;
    // Control isolates the captured-state comparison: current configs/cross-bindings
    // are internally consistent and the journal incarnation never changed.
    let fresh = NatsEventStore::connect(f.js.clone(), store.config().clone()).await?;
    assert_ne!(fresh.authority(), store.authority());
    assert_eq!(
        journal.cached_info().created,
        f.js.get_stream(store.config().stream_name())
            .await?
            .cached_info()
            .created
    );
    let before = snapshot(&f, &store).await?;
    let id = stream("new-event")?;
    configuration(
        store
            .append(&id, ExpectedVersion::NoStream, batch(&id, "direct")?)
            .await,
    );
    configuration(
        store
            .append_transaction(transaction(&f, "direct-tx")?)
            .await,
    );
    configuration(
        open_append
            .append(&id, ExpectedVersion::NoStream, batch(&id, "open")?)
            .await,
    );
    configuration(
        open_tx
            .append_transaction(transaction(&f, "open-tx")?)
            .await,
    );
    configuration(
        open_completed
            .commit_outcome(completed(&f, "open-completed")?)
            .await,
    );
    configuration(
        open_command
            .commit_outcome(completed(&f, "open-command")?)
            .await,
    );
    configuration(
        store
            .append_session()
            .await?
            .append(&id, ExpectedVersion::NoStream, batch(&id, "new")?)
            .await,
    );
    configuration(
        store
            .append_session()
            .await?
            .append_transaction(transaction(&f, "new-tx")?)
            .await,
    );
    configuration(
        store
            .append_session()
            .await?
            .commit_outcome(completed(&f, "new-completed")?)
            .await,
    );
    configuration(
        store
            .commit_outcome(completed(&f, "direct-completed")?)
            .await,
    );
    configuration(store.begin_command().await);
    let calls = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(AccessLog::default());
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
    assert_eq!(*log.0.lock().unwrap(), vec![CommandAccess::ReadOutcome; 2]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let error = store
        .lookup_outcome(marker.detail_reference().key())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), EventStoreErrorKind::ConfigurationMismatch);
    assert_eq!(error.origin(), EventStoreErrorOrigin::OutcomeRecovery);
    assert!(
        matches!(store.expire_details(marker.detail_reference().key(), marker.revision()).await,
        Err(OutcomeLifecycleError::Store(ref e)) if e.kind() == EventStoreErrorKind::ConfigurationMismatch)
    );
    assert_eq!(snapshot(&f, &store).await?, before);
    Ok(())
}

#[tokio::test]
async fn transient_state_info_outage_preserves_missing_vs_known_recovery_and_recovers() -> TestResult
{
    let mut service_kinds = Vec::new();
    for action in ["create", "add"] {
        let f = Fixture::new("rereview-state-outage").await?;
        let durable = split(&f).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let log = Arc::new(AccessLog::default());
        let original = service(&f, durable.clone(), calls.clone(), log.clone())?;
        let bus = CommandBus::new(f.context.clone(), f.adapter.clone());
        let command = bus.encode(request("known", "target", action)?)?;
        let winner = CommandProcessingService::process(&original, &caller()?, &command)
            .await
            .into_result()?;
        let proxy =
            StateProxy::start(durable.config().outcome_state_stream_name(), "unused", 0).await?;
        let store =
            Arc::new(NatsEventStore::connect(proxy.js.clone(), durable.config().clone()).await?);
        let p = Arc::new(service(&f, store.clone(), calls.clone(), log.clone())?);
        log.0.lock().unwrap().clear();
        let before = snapshot(&f, &store).await?;
        proxy.faults.block_state_info.store(true, Ordering::SeqCst);
        let unknown = OutcomeKey::new(
            store.authority().clone(),
            f.context.name().clone(),
            OperationId::new("unknown")?,
        );
        let missing = store.lookup_outcome(&unknown).await.unwrap_err();
        assert_eq!(missing.kind(), EventStoreErrorKind::Unavailable);
        assert_eq!(missing.origin(), EventStoreErrorOrigin::Backend);
        let known = store
            .lookup_outcome(winner.outcome().marker().detail_reference().key())
            .await
            .unwrap_err();
        assert_eq!(known.kind(), EventStoreErrorKind::Unavailable);
        assert_eq!(known.origin(), EventStoreErrorOrigin::OutcomeRecovery);
        let failed = CommandProcessingService::process(p.as_ref(), &caller()?, &command)
            .await
            .into_result()
            .unwrap_err();
        service_kinds.push(failed.kind());
        eprintln!("{action}: missing={missing:?}; known={known:?}; service={failed:?}");
        // The retained service-origin matrix intentionally permits a retryable
        // Unavailable diagnostic. A typed unavailable detail value is a different
        // path. Verify the exact diagnostic and native behavior, not blanket suppression.
        let diagnostic = failed.technical_response(&command)?.unwrap();
        assert_eq!(
            diagnostic.outcome(),
            &CommandResponseOutcome::ExecutionFailed(rostfrei::ExecutionFailure::Store(
                rostfrei::StoreFailureKind::Unavailable
            ))
        );
        command.validate_response(&diagnostic)?;
        let fresh_command = bus.encode(request("unknown", "new", "create")?)?;
        assert!(
            CommandProcessingService::process(p.as_ref(), &caller()?, &fresh_command)
                .await
                .into_result()
                .is_err()
        );
        assert_eq!(*log.0.lock().unwrap(), vec![CommandAccess::ReadOutcome; 2]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(proxy.faults.dropped_reads.load(Ordering::SeqCst) >= 4);
        assert_eq!(
            store.load(&stream("target")?).await?.len(),
            usize::from(action == "create")
        );
        assert_eq!(
            store.audit_history(&stream("target")?).await?,
            durable.audit_history(&stream("target")?).await?
        );
        assert_eq!(snapshot(&f, &store).await?, before);
        let worker = f
            .adapter
            .command_handler_with_resolver(p.clone(), Arc::new(ServicePrincipalContext(caller()?)));
        assert!(matches!(
            worker.handle(command_delivery(&command)?).await,
            DeliveryDisposition::RetryAfter(_)
        ));
        let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
        let published = reader
            .read_command_response(
                &command.response_address()?,
                command.operation_id(),
                command.message_id(),
                WAIT,
            )
            .await?;
        assert_eq!(published, diagnostic);
        assert_eq!(*log.0.lock().unwrap(), vec![CommandAccess::ReadOutcome; 3]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshot(&f, &store).await?, before);
        proxy.faults.block_state_info.store(false, Ordering::SeqCst);
        assert!(matches!(
            store.lookup_outcome(&unknown).await?,
            OutcomeLookup::Missing
        ));
        let replay = CommandProcessingService::process(p.as_ref(), &caller()?, &command)
            .await
            .into_result()?;
        assert_eq!(replay.outcome(), winner.outcome());
        assert_eq!(replay.origin(), rostfrei::CompletionOrigin::Replayed);
        assert_eq!(
            worker.handle(command_delivery(&command)?).await,
            DeliveryDisposition::Acknowledge
        );
        let recovered = reader
            .read_command_response(
                &command.response_address()?,
                command.operation_id(),
                command.message_id(),
                WAIT,
            )
            .await?;
        assert_eq!(&recovered, winner.admit(&caller()?, &command)?);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(snapshot(&f, &store).await?, before);
    }
    assert_eq!(
        service_kinds,
        vec![rostfrei::CommandProcessorErrorKind::Unavailable; 2]
    );
    Ok(())
}

#[tokio::test]
async fn readonly_expired_winner_keeps_integrity_reads_and_cannot_mutate_or_admit_details()
-> TestResult {
    let f = Fixture::new("rereview-readonly-expired").await?;
    let writer = split(&f).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(AccessLog::default());
    let p = service(&f, writer.clone(), calls.clone(), log.clone())?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone())
        .encode(request("winner", "target", "create")?)?;
    let winner = CommandProcessingService::process(&p, &caller()?, &command)
        .await
        .into_result()?;
    let marker = winner.outcome().marker();
    writer
        .expire_details(marker.detail_reference().key(), marker.revision())
        .await?;
    let before_connect = snapshot(&f, &writer).await?;
    let readonly = Arc::new(
        NatsEventStore::connect(
            f.js.clone(),
            writer.config().clone().with_read_only_outcomes(),
        )
        .await?
        .with_history_auditing(true),
    );
    assert_eq!(snapshot(&f, &readonly).await?, before_connect);
    let p = service(&f, readonly.clone(), calls.clone(), log.clone())?;
    log.0.lock().unwrap().clear();
    assert!(
        matches!(readonly.lookup_outcome(marker.detail_reference().key()).await?,
        OutcomeLookup::Completed(ref v) if matches!(v.details(), OutcomeDetails::Expired))
    );
    let replay = CommandProcessingService::process(&p, &caller()?, &command)
        .await
        .into_result()?;
    assert_eq!(replay.origin(), rostfrei::CompletionOrigin::Replayed);
    let error = replay.admit(&caller()?, &command).unwrap_err();
    assert_eq!(
        error.kind(),
        rostfrei::CommandProcessorErrorKind::CompletedDetailsExpired
    );
    assert!(error.technical_response(&command)?.is_none());
    assert_eq!(*log.0.lock().unwrap(), vec![CommandAccess::ReadOutcome]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(readonly.load(&stream("target")?).await?.len(), 1);
    assert_eq!(readonly.audit_history(&stream("target")?).await?.len(), 1);
    assert_eq!(
        readonly
            .audit_streams(&AggregateType::new("target")?)
            .await?
            .len(),
        1
    );
    assert_eq!(
        readonly
            .list_streams(&AggregateType::new("target")?)
            .await?
            .len(),
        1
    );
    assert!(
        readonly
            .load_transaction_receipt(&OperationId::new("winner")?)
            .await?
            .is_some()
    );
    configuration(readonly.append_session().await);
    configuration(readonly.begin_command().await);
    configuration(readonly.commit_outcome(completed(&f, "other")?).await);
    assert_eq!(
        readonly
            .expire_details(marker.detail_reference().key(), marker.revision())
            .await
            .unwrap_err(),
        OutcomeLifecycleError::Unsupported
    );
    assert_eq!(snapshot(&f, &readonly).await?, before_connect);
    Ok(())
}

#[tokio::test]
async fn writable_legacy_transactions_keep_guards_and_replay_while_readonly_clones_refuse_writes()
-> TestResult {
    let f = Fixture::new("rereview-legacy-guards").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?;
    provision_event_store(&f.js, &config).await?;
    let writer = NatsEventStore::connect(f.js.clone(), config.clone()).await?;
    let id = stream("written")?;
    let p = EventTransaction::new(
        OperationId::new("legacy-tx")?,
        ContentFingerprint::digest("event"),
        vec![
            TransactionParticipant::new(
                id.clone(),
                ExpectedVersion::NoStream,
                Some(batch(&id, "legacy-tx")?),
            ),
            TransactionParticipant::new(stream("guard")?, ExpectedVersion::NoStream, None),
        ],
    )
    .with_bounded_context(f.context.name().clone());
    let first = writer
        .append_session()
        .await?
        .append_transaction(p.clone())
        .await?;
    assert!(matches!(first, TransactionAppendOutcome::Appended(_)));
    assert!(matches!(
        writer.append_transaction(p.clone()).await?,
        TransactionAppendOutcome::ExactReplay(_)
    ));
    let before = snapshot(&f, &writer).await?;
    let readonly = NatsEventStore::connect(f.js.clone(), config.with_read_only_outcomes())
        .await?
        .with_history_auditing(true);
    let clone = readonly.clone();
    assert_eq!(clone.load(&id).await?.len(), 1);
    assert_eq!(
        clone
            .audit_streams(&AggregateType::new("target")?)
            .await?
            .len(),
        1
    );
    let receipt = clone
        .load_transaction_receipt(&OperationId::new("legacy-tx")?)
        .await?
        .unwrap();
    assert_eq!(receipt.streams().len(), 2);
    configuration(clone.append_transaction(p).await);
    configuration(clone.append_session().await);
    configuration(
        clone
            .append(&id, ExpectedVersion::NoStream, batch(&id, "legacy-tx")?)
            .await,
    );
    assert_eq!(snapshot(&f, &clone).await?, before);
    // A distinct writable handle remains usable; readonly is not global broker mutation.
    writer
        .append_session()
        .await?
        .append(
            &stream("later")?,
            ExpectedVersion::NoStream,
            batch(&stream("later")?, "later")?,
        )
        .await?;
    assert_eq!(clone.load(&stream("later")?).await?.len(), 1);
    assert!(
        !clone
            .config()
            .stream_config()
            .metadata
            .contains_key("rostfrei.completed-outcomes.epoch")
    );
    Ok(())
}
