//! Fresh independent repair review: rebound journal and unavailable-state write routes.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::too_many_lines
)]
#[allow(clippy::indexing_slicing, reason = "existing owned protocol relay")]
mod modular_state_fault_support;
mod release_contract_support;
use async_trait::async_trait;
use modular_state_fault_support::StateProxy;
use release_contract_support::*;
use rostfrei_runtime::{
    CommandAccess, CommandProcessingService, PrincipalId, TrustedCommandContext,
};
rostfrei_runtime::install_macro_support!();

fn transaction(f: &Fixture, op: &str) -> TestResult<EventTransaction> {
    Ok(EventTransaction::new(
        OperationId::new(op)?,
        ContentFingerprint::digest(op),
        vec![],
    )
    .with_bounded_context(f.context.name().clone()))
}

fn completed(f: &Fixture, op: &str) -> TestResult<EventTransaction> {
    let proposal = transaction(f, op)?;
    let evidence = CommandExecutionMetadata::new(
        proposal.operation_id().clone(),
        proposal.operation_fingerprint(),
    )
    .with_default_outcome_evidence::<()>()
    .outcome_evidence()
    .cloned();
    Ok(proposal.with_completion(CommandCompletion::new(evidence, None)))
}

fn batch(id: &StreamId, op: &str) -> TestResult<EventBatch> {
    let operation = OperationId::new(op)?;
    let commit = derive_commit_id(id, &operation);
    Ok(EventBatch::new(
        commit.clone(),
        operation,
        ContentFingerprint::digest(op),
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

fn refused<T>(route: &str, result: Result<T, EventStoreError>, kind: EventStoreErrorKind) {
    let error = result.err().unwrap();
    assert_eq!(error.kind(), kind, "{route}: {error:?}");
}

async fn snapshot(f: &Fixture, config: &NatsEventStoreConfig) -> TestResult<serde_json::Value> {
    let mut values = Vec::new();
    for name in [config.stream_name(), config.outcome_state_stream_name()] {
        let stream = f.js.get_stream(name).await?;
        values.push(serde_json::json!({
            "name": name,
            "created": stream.cached_info().created.to_string(),
            "config": stream.cached_info().config,
            "state": format!("{:?}", stream.cached_info().state),
        }));
    }
    Ok(serde_json::json!(values))
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

#[derive(Clone, Copy)]
enum Fault {
    ReboundJournal,
    UnavailableState,
}

async fn challenge(fault: Fault) -> TestResult {
    let f = Fixture::new("fresh-review-authority").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_storage_limits(2 * 1024 * 1024, 64 * 1024)?
        .with_fresh_split_outcome_epoch("fresh-review")?
        .with_explicit_detail_expiry(true)?;
    provision_event_store(&f.js, &config).await?;
    let proxy = StateProxy::start(config.outcome_state_stream_name(), "unused", 0).await?;
    let store = Arc::new(NatsEventStore::connect(proxy.js.clone(), config.clone()).await?);
    let id = stream("retained")?;
    let seed = batch(&id, "seed")?;
    store
        .append(&id, ExpectedVersion::NoStream, seed.clone())
        .await?;
    let proposal = completed(&f, "winner")?;
    let winner = store
        .begin_command()
        .await?
        .commit_outcome(proposal.clone())
        .await?;
    let marker = winner.outcome().marker();
    let append = store.append_session().await?;
    assert_eq!(append.load(&id).await?.len(), 1);
    let tx = store.append_session().await?;
    let completion = store.append_session().await?;
    let command_session = store.begin_command().await?;
    let surviving_session = store.begin_command().await?;
    let history = store.append_session().await?;
    assert_eq!(history.load(&id).await?.len(), 1);
    let expected = match fault {
        Fault::ReboundJournal => {
            let original = f.js.get_stream(config.stream_name()).await?;
            let state = f.js.get_stream(config.outcome_state_stream_name()).await?;
            f.js.delete_stream(config.stream_name()).await?;
            let replacement =
                f.js.create_stream(original.cached_info().config.clone())
                    .await?;
            assert_ne!(
                replacement.cached_info().created,
                original.cached_info().created
            );
            let mut rebound = state.cached_info().config.clone();
            rebound.metadata.insert(
                "rostfrei.split.journal-incarnation".to_owned(),
                replacement.cached_info().created.to_string(),
            );
            f.js.update_stream(rebound).await?;
            // Both current configurations and cross-bindings are valid. Only the
            // captured original journal distinguishes these handles.
            let fresh = NatsEventStore::connect(f.js.clone(), config.clone()).await?;
            assert_ne!(fresh.authority(), store.authority());
            assert_eq!(
                f.js.get_stream(config.outcome_state_stream_name())
                    .await?
                    .cached_info()
                    .created,
                state.cached_info().created
            );
            EventStoreErrorKind::ConfigurationMismatch
        }
        Fault::UnavailableState => {
            proxy.faults.block_state_info.store(true, Ordering::SeqCst);
            EventStoreErrorKind::Unavailable
        }
    };
    let before = snapshot(&f, &config).await?;
    // Challenge exact replay as well as new writes: neither can claim success
    // through an observed damaged authority, including cached session histories.
    refused(
        "direct append",
        store
            .append(&id, ExpectedVersion::NoStream, seed.clone())
            .await,
        expected,
    );
    refused(
        "direct transaction",
        store.append_transaction(transaction(&f, "direct")?).await,
        expected,
    );
    refused(
        "open append",
        append
            .append(&id, ExpectedVersion::NoStream, seed.clone())
            .await,
        expected,
    );
    refused(
        "open transaction",
        tx.append_transaction(transaction(&f, "open")?).await,
        expected,
    );
    refused(
        "new append",
        store
            .append_session()
            .await?
            .append(&id, ExpectedVersion::NoStream, seed.clone())
            .await,
        expected,
    );
    refused(
        "new transaction",
        store
            .append_session()
            .await?
            .append_transaction(transaction(&f, "new")?)
            .await,
        expected,
    );
    refused(
        "direct completion",
        store.commit_outcome(proposal.clone()).await,
        expected,
    );
    refused(
        "open completion",
        completion.commit_outcome(proposal.clone()).await,
        expected,
    );
    refused(
        "new completion",
        store
            .append_session()
            .await?
            .commit_outcome(proposal.clone())
            .await,
        expected,
    );
    refused(
        "open command",
        command_session.commit_outcome(proposal.clone()).await,
        expected,
    );
    refused("begin command", store.begin_command().await, expected);
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "missing",
        "new-target",
        "create",
    )?)?;
    let rejection = CommandResponse::rejected(
        rostfrei_runtime::command_response_message_id(command.message_id())?,
        command.message_id().clone(),
        command.address().clone(),
        command.operation_id().clone(),
        command.correlation_id().clone(),
        map_rejection(&"target-not-found")?,
    )?;
    refused(
        "import",
        store
            .import_legacy_rejected_outcome(&command, &rejection, f.messaging.topology())
            .await,
        expected,
    );
    assert!(
        matches!(store.expire_details(marker.detail_reference().key(), marker.revision()).await,
                    Err(OutcomeLifecycleError::Store(ref e)) if e.kind() == expected)
    );
    refused(
        "known lookup",
        store.lookup_outcome(marker.detail_reference().key()).await,
        expected,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let access = Arc::new(AccessLog::default());
    let mut processor = CommandProcessor::builder(f.context.clone(), store.clone())
        .authorizer(access.clone())
        .build()?;
    processor.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let caller = TrustedCommandContext::new(
        PrincipalId::new("fresh-review")?,
        SecurityScope::new("fresh-review")?,
    );
    assert!(
        processor
            .lookup(&caller, &command)
            .await
            .into_result()
            .is_err()
    );
    assert!(
        CommandProcessingService::process(&processor, &caller, &command)
            .await
            .into_result()
            .is_err()
    );
    assert_eq!(
        *access.0.lock().unwrap(),
        vec![CommandAccess::ReadOutcome; 2]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot(&f, &config).await?, before);
    match fault {
        Fault::ReboundJournal => {
            refused("cached session history", history.load(&id).await, expected);
            refused(
                "new session history",
                store.append_session().await?.load(&id).await,
                expected,
            );
            refused(
                "surviving command",
                surviving_session.commit_outcome(proposal).await,
                expected,
            );
            assert_eq!(snapshot(&f, &config).await?, before);
        }
        Fault::UnavailableState => {
            assert!(proxy.faults.dropped_reads.load(Ordering::SeqCst) >= 15);
            // Ordinary journal history remains readable through a state outage.
            assert_eq!(history.load(&id).await?.len(), 1);
            proxy.faults.block_state_info.store(false, Ordering::SeqCst);
            assert!(matches!(
                store.append(&id, ExpectedVersion::NoStream, seed).await?,
                AppendOutcome::ExactReplay(_)
            ));
            assert!(
                surviving_session
                    .commit_outcome(proposal)
                    .await?
                    .is_exact_replay()
            );
            assert_eq!(snapshot(&f, &config).await?, before);
            // An old store can recover from an outage of the SAME pair and still
            // append a genuinely new transaction under the unchanged policy.
            assert!(matches!(
                store
                    .append_transaction(transaction(&f, "recovered")?)
                    .await?,
                TransactionAppendOutcome::Appended(_)
            ));
        }
    }
    Ok(())
}

#[tokio::test]
async fn rebound_journal_cannot_be_adopted_by_captured_store_or_sessions() -> TestResult {
    Box::pin(challenge(Fault::ReboundJournal)).await
}

#[tokio::test]
async fn state_info_outage_blocks_every_write_route_and_same_pair_recovers() -> TestResult {
    Box::pin(challenge(Fault::UnavailableState)).await
}
