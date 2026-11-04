//! Authority fencing at command admission, captured sessions and maintenance.
#![allow(clippy::unwrap_used, clippy::panic_in_result_fn)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{CommandAccess, CommandProcessingService, PrincipalId, TrustedCommandContext};
rostfrei::install_macro_support!();

async fn fixture(label: &str) -> TestResult<(Fixture, Arc<NatsEventStore>)> {
    let f = Fixture::new(label).await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("authority")?
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
            .append(&id, ExpectedVersion::NoStream, batch)
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
