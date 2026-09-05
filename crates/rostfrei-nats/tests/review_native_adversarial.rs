//! Independent exact-snapshot native review. Product source remains immutable.
#![allow(
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::too_many_lines
)]
#![allow(
    clippy::items_after_statements,
    reason = "fixture-local rejection handler"
)]
mod release_contract_support;
mod review_native_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{AllowAllCommands, CommandProcessingService, PrincipalId, TrustedCommandContext};
rostfrei::install_macro_support!();

fn caller() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("review-service")?,
        SecurityScope::new("review-scope")?,
    ))
}
fn service(
    f: &Fixture,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<dyn CommandProcessingService>> {
    let mut p = CommandProcessor::builder(f.context.clone(), store)
        .authorizer(Arc::new(AllowAllCommands))
        .build()?;
    p.register::<Act, _>(Handler(calls), map_rejection)?;
    Ok(Arc::new(p))
}
fn no_events(f: &Fixture, op: &str) -> TestResult<EventTransaction> {
    Ok(EventTransaction::new(
        OperationId::new(op)?,
        ContentFingerprint::digest(op),
        vec![],
    )
    .with_bounded_context(f.context.name().clone()))
}

#[tokio::test]
async fn split_stale_low_level_writer_cannot_activate_a_recreated_unmarked_journal() -> TestResult {
    let f = Fixture::new("review-stale-compat").await?;
    let store = f.split_event_store().await?;
    let original = f.js.get_stream(store.config().stream_name()).await?;
    let old_incarnation = original.cached_info().created;
    let mut recreated = original.cached_info().config.clone();
    recreated.metadata.clear();
    f.js.delete_stream(store.config().stream_name()).await?;
    let current = f.js.create_stream(recreated).await?;
    assert_ne!(old_incarnation, current.cached_info().created);
    assert!(
        store.begin_command().await.is_err(),
        "coordinated port control must be fenced"
    );
    let result = store
        .append_transaction(no_events(&f, "stale-write")?)
        .await;
    let id = stream("stale-event")?;
    let operation = OperationId::new("stale-event-write")?;
    let commit = derive_commit_id(&id, &operation);
    let event = NewEvent::new(
        derive_event_id(&commit, 0),
        "changed",
        1,
        serde_json::to_vec(&Changed {
            created: true,
            item: None,
        })?,
    )?;
    let event_result = store
        .append(
            &id,
            ExpectedVersion::NoStream,
            EventBatch::new(
                commit,
                operation,
                ContentFingerprint::digest("stale-event"),
                vec![event],
            )?,
        )
        .await;
    let session_result = store
        .append_session()
        .await?
        .append_transaction(no_events(&f, "stale-session-write")?)
        .await;
    let count = f.messages(store.config().stream_name()).await?;
    eprintln!("stale low-level result={result:?}; recreated journal messages={count}");
    eprintln!(
        "stale direct event result={event_result:?}; newly captured legacy session={session_result:?}"
    );
    assert!(
        result.is_err(),
        "stale split EventStore compatibility writer reopened a recreated authority"
    );
    assert_eq!(count, 0);
    assert!(event_result.is_err());
    assert!(session_result.is_err());
    Ok(())
}

#[tokio::test]
async fn explicit_read_only_profile_blocks_low_level_writes_in_unmarked_legacy_authority()
-> TestResult {
    let f = Fixture::new("review-readonly-compat").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?;
    provision_event_store(&f.js, &config).await?;
    let store = NatsEventStore::connect(f.js.clone(), config.with_read_only_outcomes()).await?;
    assert!(store.begin_command().await.is_err());
    let result = store
        .append_transaction(no_events(&f, "readonly-write")?)
        .await;
    eprintln!("read-only legacy low-level result={result:?}");
    assert!(
        result.is_err(),
        "explicit replay-only handle permits a public transaction write"
    );
    assert_eq!(f.messages(store.config().stream_name()).await?, 0);
    Ok(())
}

struct AccessLog(std::sync::Mutex<Vec<rostfrei::CommandAccess>>);
#[async_trait]
impl rostfrei::CommandAuthorizer for AccessLog {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &rostfrei::ValidatedCommand,
        access: rostfrei::CommandAccess,
    ) -> Result<(), rostfrei::AuthorizationError> {
        self.0.lock().unwrap().push(access);
        Ok(())
    }
}

#[tokio::test]
async fn absent_marker_does_not_claim_authoritative_missing_after_state_authority_loss()
-> TestResult {
    let f = Fixture::new("review-missing-pair").await?;
    let store = f.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(AccessLog(std::sync::Mutex::new(Vec::new())));
    let mut p = CommandProcessor::builder(f.context.clone(), store.clone())
        .authorizer(policy.clone())
        .build()?;
    p.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "new-command",
        "target",
        "create",
    )?)?;
    assert!(matches!(
        CommandProcessingService::lookup(&p, &caller()?, &command)
            .await
            .into_result()?,
        OutcomeLookup::Missing
    ));
    policy.0.lock().unwrap().clear();
    f.js.delete_stream(store.config().outcome_state_stream_name())
        .await?;
    let lookup = CommandProcessingService::lookup(&p, &caller()?, &command)
        .await
        .into_result();
    let processing = CommandProcessingService::process(&p, &caller()?, &command)
        .await
        .into_result();
    assert!(processing.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    eprintln!(
        "lookup after authority loss={lookup:?}; access phases={:?}",
        policy.0.lock().unwrap()
    );
    assert!(
        lookup.is_err(),
        "lost paired authority was reported as eligible Missing"
    );
    assert!(
        !policy
            .0
            .lock()
            .unwrap()
            .contains(&rostfrei::CommandAccess::Execute)
    );
    Ok(())
}

#[tokio::test]
async fn real_state_capacity_blocks_decision_and_preserves_an_earlier_winner() -> TestResult {
    let f = Fixture::new("review-real-state-capacity").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("review-capacity")?
        .with_storage_limits(16_384, 4096)?;
    provision_event_store(&f.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(&f, store.clone(), calls.clone())?;
    let bus = CommandBus::new(f.context.clone(), f.adapter.clone());
    let first = bus.encode(request("retained", "target", "noop")?)?;
    let winner = service.process(&caller()?, &first).await.into_result()?;
    let state_name = store.config().outcome_state_stream_name();
    let original_config =
        f.js.get_stream(state_name)
            .await?
            .cached_info()
            .config
            .clone();
    let mut filled = 0;
    for index in 0..100 {
        // Owned candidate-like subjects consume capacity; no config mutation or eviction.
        let mut h = async_nats::HeaderMap::new();
        h.insert("Nats-Expected-Stream", state_name);
        h.insert("Nats-Expected-Last-Subject-Sequence", "0");
        let result =
            f.js.publish_with_headers(
                format!("{}.outcome.fill{index}", store.config().subject_prefix()),
                h,
                vec![b'x'; 3900].into(),
            )
            .await?
            .await;
        if result.is_err() {
            break;
        }
        filled += 1;
    }
    assert!(filled > 0 && filled < 100);
    let before = f.messages(store.config().stream_name()).await?;
    // A larger bounded mapped rejection cannot fit in the remaining state capacity.
    struct Large;
    #[async_trait]
    impl CommandHandler<()> for Large {
        type Rejection = String;
        async fn handle(
            &self,
            (): &(),
            _: &mut CommandExecution<'_>,
        ) -> CommandHandlingResult<String> {
            Ok(CommandDecision::Rejected("x".repeat(1900)))
        }
    }
    let result = CommandExecutor::new(store.clone())
        .with_max_conflict_retries(0)
        .execute_with_rejection_codec(
            &Large,
            CommandExecutionMetadata::new(
                OperationId::new("capacity-rejected")?,
                ContentFingerprint::digest("capacity-rejected"),
            )
            .with_bounded_context(f.context.name().clone()),
            &(),
            &JsonRejectionCodec::new("review", 1),
        )
        .await;
    eprintln!("real state capacity result={result:?}; filler count={filled}");
    assert!(
        matches!(result, Err(CommandExecutionError::RejectedOutcomePersistence(ref e)) if e.kind() == EventStoreErrorKind::CapacityExhausted)
    );
    assert_eq!(f.messages(store.config().stream_name()).await?, before);
    assert_eq!(
        f.js.get_stream(state_name).await?.cached_info().config,
        original_config
    );
    let replay = service.process(&caller()?, &first).await.into_result()?;
    assert_eq!(winner.outcome(), replay.outcome());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct ForbiddenCold(AtomicUsize);

#[tokio::test]
async fn native_strict_proposal_replay_survives_expiry_without_using_command_identity_as_equality()
-> TestResult {
    let f = Fixture::new("review-proposal-expiry").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("review-proposal")?
        .with_explicit_detail_expiry(true)?;
    provision_event_store(&f.js, &config).await?;
    let store = NatsEventStore::connect(f.js.clone(), config).await?;
    let completion = CommandCompletion::new(
        CommandExecutionMetadata::new(
            OperationId::new("proposal")?,
            ContentFingerprint::digest("proposal"),
        )
        .with_default_outcome_evidence::<()>()
        .outcome_evidence()
        .cloned(),
        Some(JsonRejectionCodec::new("review", 1).encode(&"original rejection".to_owned())?),
    );
    let make = |expected, completion| {
        no_events(&f, "proposal").map(|p| {
            EventTransaction::new(
                p.operation_id().clone(),
                p.operation_fingerprint(),
                vec![TransactionParticipant::new(
                    stream("guard").unwrap(),
                    expected,
                    None,
                )],
            )
            .with_bounded_context(f.context.name().clone())
            .with_completion(completion)
        })
    };
    let original = make(ExpectedVersion::NoStream, completion.clone())?;
    let first = store
        .begin_command()
        .await?
        .commit_outcome(original)
        .await?;
    let marker = first.outcome().marker();
    let altered_base = make(
        ExpectedVersion::Exact(StreamVersion::new(999)),
        completion.clone(),
    )?;
    assert!(
        store
            .begin_command()
            .await?
            .commit_outcome(altered_base.clone())
            .await?
            .is_exact_replay()
    );
    let count = f.messages(store.config().stream_name()).await?;
    store
        .expire_details(marker.detail_reference().key(), marker.revision())
        .await?;
    let replay = store
        .begin_command()
        .await?
        .commit_outcome(altered_base)
        .await?;
    assert!(replay.is_exact_replay());
    assert_eq!(replay.outcome().marker(), marker);
    assert!(matches!(
        replay.outcome().details(),
        OutcomeDetails::Expired
    ));
    let mut changed = completion;
    changed.rejection =
        Some(JsonRejectionCodec::new("review", 1).encode(&"different rejection".to_owned())?);
    assert_eq!(
        store
            .begin_command()
            .await?
            .commit_outcome(make(ExpectedVersion::NoStream, changed)?)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::IdentityConflict
    );
    assert!(
        store
            .load_transaction_receipt(&OperationId::new("proposal")?)
            .await
            .is_err(),
        "expired rejection must never become an acceptance receipt"
    );
    assert_eq!(f.messages(store.config().stream_name()).await?, count);
    assert!(store.audit_history(&stream("guard")?).await?.is_empty());
    Ok(())
}

struct RejectPolicy {
    failure: rostfrei::AuthorizationError,
    phase: rostfrei::CommandAccess,
}
#[async_trait]
impl rostfrei::CommandAuthorizer for RejectPolicy {
    async fn authorize(
        &self,
        _: &TrustedCommandContext,
        _: &rostfrei::ValidatedCommand,
        access: rostfrei::CommandAccess,
    ) -> Result<(), rostfrei::AuthorizationError> {
        if access == self.phase {
            Err(self.failure)
        } else {
            Ok(())
        }
    }
}
#[tokio::test]
async fn native_worker_authorization_failures_never_publish_shared_caller_specific_diagnostics()
-> TestResult {
    use rostfrei::{AuthorizationError, CommandAccess};
    for phase in [CommandAccess::ReadOutcome, CommandAccess::Execute] {
        for failure in [
            AuthorizationError::Denied,
            AuthorizationError::Unauthenticated,
            AuthorizationError::Unavailable,
            AuthorizationError::InvalidConfiguration,
        ] {
            let f = Fixture::new("review-worker-auth").await?;
            let store = f.split_event_store().await?;
            let calls = Arc::new(AtomicUsize::new(0));
            let mut p = CommandProcessor::builder(f.context.clone(), store.clone())
                .authorizer(Arc::new(RejectPolicy { failure, phase }))
                .build()?;
            p.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
            let command = CommandBus::new(f.context.clone(), f.adapter.clone())
                .encode(request("refused", "target", "create")?)?;
            let worker = f.adapter.command_handler_with_resolver(
                Arc::new(p),
                Arc::new(ServicePrincipalContext(caller()?)),
            );
            let result = worker.handle(command_delivery(&command)?).await;
            assert_eq!(
                matches!(result, DeliveryDisposition::RetryAfter(_)),
                failure == AuthorizationError::Unavailable,
                "{failure:?}/{phase:?}: {result:?}"
            );
            if failure != AuthorizationError::Unavailable {
                assert!(matches!(result, DeliveryDisposition::Quarantine(_)));
            }
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(f.messages(store.config().stream_name()).await?, 0);
            assert_eq!(
                f.messages(f.messaging.topology().command_response_stream().as_str())
                    .await?,
                0
            );
            if phase == CommandAccess::ReadOutcome {
                let allowed = service(&f, store.clone(), calls.clone())?;
                let winner = allowed.process(&caller()?, &command).await.into_result()?;
                let response = winner.admit(&caller()?, &command)?;
                f.publisher()
                    .publish_command_response(OutboundMessage::json(
                        command.response_address()?,
                        response.message_id().clone(),
                        response,
                    )?)
                    .await?;
                assert!(!matches!(
                    worker.handle(command_delivery(&command)?).await,
                    DeliveryDisposition::Acknowledge
                ));
                assert_eq!(
                    f.messages(f.messaging.topology().command_response_stream().as_str())
                        .await?,
                    1
                );
                let raw =
                    NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone())
                        .read_command_response(
                            &command.response_address()?,
                            command.operation_id(),
                            command.message_id(),
                            WAIT,
                        )
                        .await?;
                assert_eq!(&raw, response);
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
        }
    }
    Ok(())
}

#[async_trait]
impl OutcomeDetailReader for ForbiddenCold {
    async fn get(&self, _: &OutcomeDetailReference) -> Result<DetailRead, DetailReadError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(DetailReadError::Unavailable)
    }
}

#[tokio::test]
async fn malformed_current_state_blocks_cold_sender_and_expiry_without_another_evaluation()
-> TestResult {
    let f = Fixture::new("review-malformed-state").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("review-malformed")?
        .with_explicit_detail_expiry(true)?;
    provision_event_store(&f.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let original = service(&f, store.clone(), calls.clone())?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone())
        .encode(request("winner", "target", "create")?)?;
    let winner = original.process(&caller()?, &command).await.into_result()?;
    let response = winner.admit(&caller()?, &command)?.clone();
    f.publisher()
        .publish_command_response(OutboundMessage::json(
            command.response_address()?,
            response.message_id().clone(),
            &response,
        )?)
        .await?;
    let proxy = review_native_support::Proxy::start(
        store.config().outcome_state_stream_name(),
        store.config().stream_name(),
    )
    .await?;
    let maintenance =
        Arc::new(NatsEventStore::connect(proxy.js.clone(), store.config().clone()).await?);
    let cold = Arc::new(ForbiddenCold(AtomicUsize::new(0)));
    let guarded = service(
        &f,
        Arc::new(
            maintenance
                .as_ref()
                .clone()
                .with_detail_readers(cold.clone(), Some(cold.clone())),
        ),
        calls.clone(),
    )?;
    let bus = CommandBus::new(
        f.context.clone(),
        Arc::new(
            NatsMessagingAdapter::new(
                f.publisher(),
                NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone()),
            )
            .with_outcome_service(guarded.clone(), caller()?)
            .with_response_timeout(WAIT),
        ),
    );
    let journal = f.messages(store.config().stream_name()).await?;
    let state_last =
        f.js.get_stream(store.config().outcome_state_stream_name())
            .await?
            .cached_info()
            .state
            .last_sequence;
    for fault in [
        "subject",
        "sequence",
        "version",
        "binding",
        "detail-digest",
        "unknown",
        "missing-kind",
        "cas",
        "expected-stream",
        "message-id",
    ] {
        *proxy.faults.mutation.lock().unwrap() = Some(fault);
        let before = proxy.faults.altered.load(Ordering::SeqCst);
        assert!(
            guarded
                .process(&caller()?, &command)
                .await
                .into_result()
                .is_err(),
            "{fault}"
        );
        assert!(
            bus.dispatch_encoded(command.clone()).await.is_err(),
            "{fault}"
        );
        assert!(
            maintenance
                .expire_details(
                    winner.outcome().marker().detail_reference().key(),
                    winner.outcome().marker().revision()
                )
                .await
                .is_err(),
            "{fault}"
        );
        assert!(
            proxy.faults.altered.load(Ordering::SeqCst) >= before + 3,
            "{fault} did not alter real broker replies"
        );
        assert_eq!(cold.0.load(Ordering::SeqCst), 0, "{fault}");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{fault}");
        assert_eq!(f.messages(store.config().stream_name()).await?, journal);
        assert_eq!(
            f.js.get_stream(store.config().outcome_state_stream_name())
                .await?
                .cached_info()
                .state
                .last_sequence,
            state_last
        );
    }
    *proxy.faults.mutation.lock().unwrap() = None;
    assert!(matches!(
        maintenance
            .lookup_outcome(winner.outcome().marker().detail_reference().key())
            .await?,
        OutcomeLookup::Completed(_)
    ));
    store
        .expire_details(
            winner.outcome().marker().detail_reference().key(),
            winner.outcome().marker().revision(),
        )
        .await?;
    for fault in ["expired-revision", "expired-previous"] {
        *proxy.faults.mutation.lock().unwrap() = Some(fault);
        assert!(
            maintenance
                .lookup_outcome(winner.outcome().marker().detail_reference().key())
                .await
                .is_err(),
            "{fault}"
        );
    }
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    proxy.stop().await;
    Ok(())
}

#[tokio::test]
async fn native_post_durable_corruption_is_outcome_recovery_for_acceptance_and_rejection()
-> TestResult {
    for action in ["create", "add"] {
        let f = Fixture::new("review-post-durable").await?;
        let store = f.split_event_store().await?;
        let proxy = review_native_support::Proxy::start(
            store.config().outcome_state_stream_name(),
            store.config().stream_name(),
        )
        .await?;
        let proxied =
            Arc::new(NatsEventStore::connect(proxy.js.clone(), store.config().clone()).await?);
        let calls = Arc::new(AtomicUsize::new(0));
        let service = service(&f, proxied, calls.clone())?;
        let command = CommandBus::new(f.context.clone(), f.adapter.clone())
            .encode(request("durable", "target", action)?)?;
        proxy.faults.after_commit.store(true, Ordering::SeqCst);
        let error = service
            .process(&caller()?, &command)
            .await
            .into_result()
            .unwrap_err();
        assert_eq!(
            error.kind(),
            rostfrei::CommandProcessorErrorKind::CompletedDetailsBlocked,
            "{action}: {error:?}"
        );
        assert!(error.technical_response(&command)?.is_none());
        assert!(!proxy.faults.after_commit.load(Ordering::SeqCst));
        assert!(proxy.faults.altered.load(Ordering::SeqCst) > 0);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            service
                .process(&caller()?, &command)
                .await
                .into_result()
                .unwrap_err()
                .kind(),
            error.kind()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        *proxy.faults.mutation.lock().unwrap() = None;
        let recovered = service.process(&caller()?, &command).await.into_result()?;
        assert_eq!(recovered.origin(), rostfrei::CompletionOrigin::Replayed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.load(&stream("target")?).await?.len(),
            usize::from(action == "create")
        );
        proxy.stop().await;
    }
    Ok(())
}

#[tokio::test]
async fn actual_journal_capacity_with_open_response_sink_keeps_85_diagnostic_and_no_business_decision()
-> TestResult {
    let f = Fixture::new("review-journal-capacity").await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("review-journal-capacity")?
        .with_storage_limits(16_384, 4096)?;
    provision_event_store(&f.js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(f.js.clone(), config).await?);
    let mut exhausted = false;
    for i in 0..100 {
        match store
            .append_transaction(no_events(&f, &format!("fill-{i}"))?)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                assert_eq!(e.kind(), EventStoreErrorKind::CapacityExhausted);
                exhausted = true;
                break;
            }
        }
    }
    assert!(exhausted);
    let calls = Arc::new(AtomicUsize::new(0));
    let p = processor(&f.context, store.clone(), calls.clone(), None)?;
    let worker = f.adapter.command_handler(p);
    let command = CommandBus::new(f.context.clone(), f.adapter.clone()).encode(request(
        "capacity-acceptance",
        "target",
        "create",
    )?)?;
    let before = f.messages(store.config().stream_name()).await?;
    let disposition = worker.handle(command_delivery(&command)?).await;
    assert!(
        matches!(disposition, DeliveryDisposition::Quarantine(_)),
        "{disposition:?}"
    );
    let response = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone())
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            WAIT,
        )
        .await?;
    assert_eq!(
        response.outcome(),
        &CommandResponseOutcome::ExecutionFailed(rostfrei::ExecutionFailure::Store(
            rostfrei::StoreFailureKind::CapacityExhausted
        ))
    );
    command.validate_response(&response)?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(f.messages(store.config().stream_name()).await?, before);
    assert!(
        store
            .load_transaction_receipt(&OperationId::new(command.operation_id().as_str())?)
            .await?
            .is_none()
    );
    assert!(store.load(&stream("target")?).await?.is_empty());
    // Preparation stays retained, without journal authority, after capacity rejects commit.
    assert_eq!(
        f.messages(store.config().outcome_state_stream_name())
            .await?,
        1
    );
    Ok(())
}
