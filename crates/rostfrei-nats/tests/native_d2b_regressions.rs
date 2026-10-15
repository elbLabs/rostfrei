#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing
)]
mod modular_state_fault_support;
mod release_contract_support;
use modular_state_fault_support::StateProxy;
use release_contract_support::*;
use rostfrei::{
    AllowAllCommands, CommandProcessingService, CommandProcessorErrorKind, PrincipalId,
    TrustedCommandContext,
};
rostfrei::install_macro_support!();

fn caller(scope: &str) -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("native-regression")?,
        SecurityScope::new(scope)?,
    ))
}
fn service(
    f: &Fixture,
    backend: Arc<dyn CommandBackend>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<dyn CommandProcessingService>> {
    let mut processor = CommandProcessor::builder(f.context.clone(), backend)
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    processor.register::<Act, _>(Handler(calls), map_rejection)?;
    Ok(Arc::new(processor))
}

#[tokio::test]
async fn uncertain_initial_prepare_ack_reconciles_exact_bytes_before_journal_commit() -> TestResult
{
    let f = Fixture::new("d2b-prepare-ack").await?;
    let store = f.split_event_store().await?;
    let proxy = StateProxy::start(
        store.config().outcome_state_stream_name(),
        &format!("{}.outcome.*", store.config().subject_prefix()),
        0,
    )
    .await?;
    proxy.faults.lose_expiry_acks.store(true, Ordering::SeqCst);
    let proxied = Arc::new(
        NatsEventStore::connect(
            proxy.js.clone(),
            store
                .config()
                .clone()
                .with_puback_timeout(Duration::from_millis(200))?,
        )
        .await?,
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(&f, proxied, calls.clone())?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "lost-prepare",
        "target",
        "create",
    )?)?;
    let result = service
        .process(&caller("scope")?, &command)
        .await
        .into_result()?;
    assert_eq!(
        result.admit(&caller("scope")?, &command)?.outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(proxy.faults.dropped_acks.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.messages(store.config().outcome_state_stream_name())
            .await?,
        1
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    let replay = service
        .process(&caller("scope")?, &command)
        .await
        .into_result()?;
    assert_eq!(result.outcome(), replay.outcome());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn legacy_schema_three_requires_fixed_scope_mapping_and_never_supports_split_expiry()
-> TestResult {
    let f = Fixture::new("d2b-legacy-scope").await?;
    let legacy = f.event_store().await?; // Preserved inline fixture, never changed to split.
    let memory = processor(
        &f.context,
        Arc::new(InMemoryEventStore::new()),
        Arc::new(AtomicUsize::new(0)),
        None,
    )?;
    let command = local_bus(&f.context, memory.clone()).encode(request(
        "original-legacy",
        "missing",
        "add",
    )?)?;
    let response = memory.process(&command).await?;
    legacy
        .commit_outcome(command.legacy_rejection_transaction(&response)?)
        .await?;
    let journal = f.js.get_stream(legacy.config().stream_name()).await?;
    let raw = journal
        .get_raw_message(journal.cached_info().state.last_sequence)
        .await?;
    let wire: serde_json::Value = serde_json::from_slice(&raw.payload)?;
    assert_eq!(wire["schemaVersion"], 3);
    assert!(wire["receipt"]["completion"].is_object());
    let calls = Arc::new(AtomicUsize::new(0));
    let blocked = service(&f, legacy.clone(), calls.clone())?;
    assert_eq!(
        blocked
            .process(&caller("a")?, &command)
            .await
            .into_result()
            .unwrap_err()
            .kind(),
        CommandProcessorErrorKind::IdentityConflict
    );
    let mapped = Arc::new(
        NatsEventStore::connect(
            f.js.clone(),
            legacy
                .config()
                .clone()
                .with_legacy_security_scope(SecurityScope::new("a")?),
        )
        .await?,
    );
    let allowed = service(&f, mapped.clone(), calls.clone())?;
    let original = allowed
        .process(&caller("a")?, &command)
        .await
        .into_result()?;
    assert_eq!(original.admit(&caller("a")?, &command)?, &response);
    assert_eq!(
        allowed
            .lookup(&caller("b")?, &command)
            .await
            .into_result()
            .unwrap_err()
            .kind(),
        CommandProcessorErrorKind::IdentityConflict
    );
    assert_eq!(
        mapped
            .expire_details(
                original.outcome().marker().detail_reference().key(),
                original.outcome().marker().revision()
            )
            .await
            .unwrap_err(),
        OutcomeLifecycleError::Unsupported
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.messages(legacy.config().stream_name()).await?, 1);
    // A caller cannot activate legacy writes through the new coordinated port.
    assert!(mapped.begin_command().await.is_err());
    Ok(())
}

#[tokio::test]
async fn current_state_config_faults_block_completed_admission_and_new_evaluation() -> TestResult {
    let f = Fixture::new("d2b-config-drift").await?;
    let store = f.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(&f, store.clone(), calls.clone())?;
    let bus = CommandBus::new(f.context.clone(), f.adapter.clone());
    let command = bus.encode(request("winner", "target", "create")?)?;
    service
        .process(&caller("scope")?, &command)
        .await
        .into_result()?;
    let state =
        f.js.get_stream(store.config().outcome_state_stream_name())
            .await?;
    let original = state.cached_info().config.clone();
    let journal_before = f.messages(store.config().stream_name()).await?;
    for fault in ["direct", "per-subject", "capacity", "ttl", "discard"] {
        let mut changed = original.clone();
        match fault {
            "direct" => changed.allow_direct = true,
            "per-subject" => changed.max_messages_per_subject = 2,
            "capacity" => changed.max_bytes *= 2,
            "ttl" => changed.max_age = Duration::from_hours(24),
            _ => changed.discard = async_nats::jetstream::stream::DiscardPolicy::Old,
        }
        f.js.update_stream(changed).await?;
        assert!(
            service
                .lookup(&caller("scope")?, &command)
                .await
                .into_result()
                .is_err(),
            "{fault}"
        );
        let fresh = bus.encode(request(&format!("new-{fault}"), "target", "create")?)?;
        assert!(
            service
                .process(&caller("scope")?, &fresh)
                .await
                .into_result()
                .is_err(),
            "{fault}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.messages(store.config().stream_name()).await?,
            journal_before
        );
        f.js.update_stream(original.clone()).await?;
    }
    assert!(
        service
            .lookup(&caller("scope")?, &command)
            .await
            .into_result()
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn fresh_native_response_is_admitted_through_current_authorized_winner() -> TestResult {
    let f = Fixture::new("d2b-fresh-admission").await?;
    let store = f.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(&f, store.clone(), calls.clone())?;
    let who = caller("scope")?;
    let worker = Arc::new(f.adapter.command_handler_with_resolver(
        service.clone(),
        Arc::new(ServicePrincipalContext(who.clone())),
    ));
    let remote = CommandBus::new(
        f.context.clone(),
        Arc::new(
            NatsMessagingAdapter::new(
                f.publisher(),
                NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone()),
            )
            .with_outcome_service(service.clone(), who.clone())
            .with_response_timeout(WAIT),
        ),
    );
    let command = remote.encode(request("fresh-admission", "target", "create")?)?;
    assert!(matches!(
        service.lookup(&who, &command).await.into_result()?,
        OutcomeLookup::Missing
    ));
    let publish = async {
        let commands =
            f.js.get_stream(f.messaging.topology().command_stream().as_str())
                .await?;
        tokio::time::timeout(WAIT, async {
            loop {
                if commands
                    .get_last_raw_message_by_subject(command.address().as_str())
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(
            worker.handle(command_delivery(&command)?).await,
            DeliveryDisposition::Acknowledge
        );
        TestResult::Ok(())
    };
    let (result, delivered) = tokio::join!(remote.dispatch_encoded(command.clone()), publish);
    delivered?;
    assert_eq!(
        result?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}
