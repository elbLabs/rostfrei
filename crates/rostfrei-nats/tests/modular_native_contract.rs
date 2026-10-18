//! Future-API native acceptance. Every broker is owned/authenticated via the release runner.
#![allow(
    clippy::too_many_lines,
    clippy::redundant_pub_crate,
    clippy::if_not_else,
    clippy::ignored_unit_patterns,
    clippy::items_after_statements,
    clippy::panic_in_result_fn,
    clippy::unwrap_used,
    clippy::indexing_slicing
)]
mod modular_native_delta;
mod modular_restart_support;
mod modular_state_fault_support;
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{
    AuthorizationError, CommandAccess, CommandAuthorizer, CommandProcessingService, PrincipalId,
    SecurityScope, TrustedCommandContext, ValidatedCommand,
};
rostfrei::install_macro_support!();

async fn split_store(fixture: &Fixture) -> TestResult<Arc<NatsEventStore>> {
    let config = NatsEventStoreConfig::for_bounded_context(&fixture.context)?
        .with_fresh_split_outcome_epoch("modular-test-only-v2")?
        .with_explicit_detail_expiry(true)?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?;
    provision_event_store(&fixture.js, &config).await?;
    Ok(Arc::new(
        NatsEventStore::connect(fixture.js.clone(), config).await?,
    ))
}
fn caller() -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new("verified-service")?,
        SecurityScope::new("test-realm")?,
    ))
}
fn key(store: &NatsEventStore, context: &BoundedContext, operation: &OperationId) -> OutcomeKey {
    OutcomeKey::new(
        store.authority().clone(),
        context.name().clone(),
        operation.clone(),
    )
}
async fn message_count(fixture: &Fixture, name: &str) -> TestResult<u64> {
    fixture.messages(name).await
}

struct PrivateEvent(tokio::sync::mpsc::Sender<EventId>);
#[async_trait]
impl DomainEventHandler<Changed> for PrivateEvent {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, Changed>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0
            .send(event.recorded().event_id().clone())
            .await
            .map_err(|e| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    e.to_string(),
                )
            })
    }
}
async fn deliver_after_expiry(
    fixture: &Fixture,
    store: &NatsEventStore,
    event: &RecordedEvent,
) -> TestResult {
    let config = NatsDomainEventConsumerConfig::new(
        fixture.context.consumer_name("after-expiry", 1)?,
        fixture.context.durable_name("after-expiry", 1)?,
        Duration::from_secs(5),
        Duration::from_secs(2),
        RetryDelay::new(Duration::from_millis(25))?,
    )?;
    provision_domain_event_consumer(&fixture.js, store.config(), &config).await?;
    let (send, mut receive) = tokio::sync::mpsc::channel(4);
    let mut codecs = AggregateCodecs::default();
    codecs.register_json::<Target>()?;
    let mut dispatcher = DomainEventDispatcher::with_codecs(Arc::new(codecs));
    dispatcher.register::<Target, Changed, _>("changed", Arc::new(PrivateEvent(send)))?;
    let consumer = NatsDomainEventConsumer::connect(
        fixture.js.clone(),
        store.config().clone(),
        config,
        Arc::new(dispatcher),
    )
    .await?;
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let running = consumer.run_until_shutdown(receiver);
    tokio::pin!(running);
    tokio::time::timeout(WAIT, async {
        tokio::select! {
            delivered = receive.recv() => { assert_eq!(delivered.as_ref(), Some(event.event_id())); }
            result = &mut running => { return Err(format!("private-event consumer exited before delivery: {result:?}").into()); }
        }
        shutdown.send(true)?;
        running.await?;
        TestResult::Ok(())
    }).await??;
    Ok(())
}

struct Policy {
    deny_read: AtomicBool,
    principals: std::sync::Mutex<Vec<String>>,
}
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        _: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.principals
            .lock()
            .unwrap()
            .push(caller.principal().as_str().to_owned());
        if access == CommandAccess::ReadOutcome && self.deny_read.load(Ordering::SeqCst) {
            Err(AuthorizationError::Denied)
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn native_expiry_replaces_active_preserves_integrity_and_blocks_cached_business_response()
-> TestResult {
    for reject in [false, true] {
        let fixture = Fixture::new(if reject {
            "modular-expire-reject"
        } else {
            "modular-expire-accept"
        })
        .await?;
        let store = split_store(&fixture).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let policy = Arc::new(Policy {
            deny_read: AtomicBool::new(false),
            principals: std::sync::Mutex::new(Vec::new()),
        });
        let mut processor = CommandProcessor::builder(fixture.context.clone(), store.clone())
            .authorizer(policy.clone())
            .build()?;
        processor.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
        let service: Arc<dyn CommandProcessingService> = Arc::new(processor);
        let command =
            CommandBus::new(fixture.context.clone(), fixture.adapter.clone()).encode(request(
                "expire-native",
                "target",
                if reject { "add" } else { "create" },
            )?)?;
        let completed = service.process(&caller()?, &command).await.into_result()?;
        let OutcomeDetails::Available(response) = completed.outcome().details() else {
            return Err("fresh full result unavailable".into());
        };
        fixture
            .publisher()
            .publish_command_response(
                OutboundMessage::new(
                    command.response_address()?,
                    response.message_id().clone(),
                    serde_json::to_vec(response)?,
                )?
                .with_correlation_id(command.correlation_id().clone()),
            )
            .await?;
        let admitted = NatsMessagingAdapter::new(
            fixture.publisher(),
            NatsCommandResponseReader::new(
                fixture.js.clone(),
                fixture.messaging.topology().clone(),
            ),
        )
        .with_outcome_service(service.clone(), caller()?)
        .with_response_timeout(Duration::from_secs(2));
        let bus = CommandBus::new(fixture.context.clone(), Arc::new(admitted));
        assert_eq!(
            bus.dispatch_encoded(command.clone()).await?.response(),
            response
        );
        policy.deny_read.store(true, Ordering::SeqCst);
        assert_eq!(
            bus.dispatch_encoded(command.clone())
                .await
                .unwrap_err()
                .authorization_error(),
            Some(&AuthorizationError::Denied)
        );
        policy.deny_read.store(false, Ordering::SeqCst);
        let history = store.load(&stream("target")?).await?;
        let journal_before = message_count(&fixture, store.config().stream_name()).await?;
        let detail_name = store.config().outcome_state_stream_name();
        assert_eq!(message_count(&fixture, detail_name).await?, 1);
        let mut journal = fixture.js.get_stream(store.config().stream_name()).await?;
        let manifest_sequence = journal.info().await?.state.last_sequence;
        let manifest = journal.get_raw_message(manifest_sequence).await?;
        let wire: serde_json::Value = serde_json::from_slice(&manifest.payload)?;
        assert_eq!(wire["schemaVersion"], 4);
        assert!(
            wire["receipt"].get("completion").is_none(),
            "schema 4 must not embed legacy completion"
        );
        assert!(
            journal.cached_info().config.deny_delete && journal.cached_info().config.deny_purge
        );
        let mut details = fixture.js.get_stream(detail_name).await?;
        assert_ne!(
            details.cached_info().config.name,
            journal.cached_info().config.name
        );
        modular_native_delta::assert_state_config(&details.cached_info().config);
        let active = modular_native_delta::capture_latest(&mut details).await?;
        let outcome_key = key(
            &store,
            &fixture.context,
            &OperationId::new(command.operation_id().as_str())?,
        );
        let expiry = store
            .expire_details(&outcome_key, completed.outcome().marker().revision())
            .await?;
        assert_eq!(expiry.reclamation(), ReclamationStatus::Reclaimed);
        let expired = modular_native_delta::assert_replaced(&details, &active).await?;
        let retry = store
            .expire_details(&outcome_key, completed.outcome().marker().revision())
            .await?;
        assert_eq!(retry.reclamation(), ReclamationStatus::Reclaimed);
        assert_eq!(
            message_count(&fixture, detail_name).await?,
            1,
            "one retained Expired state, with the old Active sequence absent"
        );
        assert_eq!(
            modular_native_delta::assert_replaced(&details, &active).await?,
            expired,
            "duplicate expiry must reuse the same terminal record"
        );
        assert_eq!(
            message_count(&fixture, store.config().stream_name()).await?,
            journal_before
        );
        assert_eq!(
            journal.get_raw_message(manifest_sequence).await?.payload,
            manifest.payload
        );
        let fresh = NatsEventStore::connect(
            fixture.js.clone(),
            store.config().clone().with_explicit_detail_expiry(false)?,
        )
        .await?;
        assert!(
            matches!(fresh.lookup_outcome(&outcome_key).await?, OutcomeLookup::Completed(outcome) if matches!(outcome.details(), OutcomeDetails::Expired))
        );
        assert_eq!(fresh.load(&stream("target")?).await?, history);
        assert_eq!(fresh.audit_history(&stream("target")?).await?, history);
        assert_eq!(
            fresh
                .audit_streams(&AggregateType::new("target")?)
                .await?
                .len(),
            usize::from(!reject)
        );
        if let Some(event) = history.first() {
            deliver_after_expiry(&fixture, &fresh, event).await?;
        }
        let error = bus.dispatch_encoded(command.clone()).await.unwrap_err();
        assert_eq!(
            error.kind(),
            rostfrei::CommandBusErrorKind::CompletedDetailsExpired
        );
        // Raw transport projection remains intact: the admission gate, not accidental cache
        // eviction or worker-side reconciliation, is what prevented disclosure.
        let raw = NatsCommandResponseReader::new(
            fixture.js.clone(),
            fixture.messaging.topology().clone(),
        )
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(1),
        )
        .await?;
        assert_eq!(&raw, response);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

struct VerifyDelivery {
    calls: AtomicUsize,
}
#[async_trait]
impl TrustedCommandContextResolver for VerifyDelivery {
    async fn resolve(
        &self,
        delivery: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        // An application-defined verifier fixture; this is deliberately NOT a JWT/FGA protocol.
        // The principal/tenant headers are untrusted and never used as identity.
        if delivery.metadata().get("application-proof") != Some("fixture-verified-capability") {
            return Err(AuthorizationError::Unauthenticated);
        }
        Ok(caller().unwrap())
    }
}
#[tokio::test]
async fn native_worker_uses_verified_context_even_when_a_shared_response_already_exists()
-> TestResult {
    let fixture = Fixture::new("modular-native-resolver").await?;
    let store = split_store(&fixture).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy {
        deny_read: AtomicBool::new(false),
        principals: std::sync::Mutex::new(Vec::new()),
    });
    let mut processor = CommandProcessor::builder(fixture.context.clone(), store.clone())
        .authorizer(policy.clone())
        .build()?;
    processor.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
    let resolver = Arc::new(VerifyDelivery {
        calls: AtomicUsize::new(0),
    });
    let handler = fixture
        .adapter
        .command_handler_with_resolver(Arc::new(processor), resolver.clone());
    let command = CommandBus::new(fixture.context.clone(), fixture.adapter.clone())
        .encode(request("verified-native", "target", "create")?)?;
    let delivery = |valid: bool| -> TestResult<MessageDelivery<CommandAddress>> {
        let mut metadata = CallerMetadata::try_from_entries([
            ("principal", "spoof-admin"),
            ("tenant", "spoof-tenant"),
        ])?;
        if valid {
            metadata.insert("application-proof", "fixture-verified-capability")?;
        }
        Ok(MessageDelivery::new_with_transport_context(
            command.address().clone(),
            command.message_id().clone(),
            command.payload().to_vec(),
            metadata,
            Some(command.correlation_id().clone()),
            None,
            DeliveryInfo::new(1, 0, 1, 1)?,
        )?)
    };
    assert!(!matches!(
        handler.handle(delivery(false)?).await,
        DeliveryDisposition::Acknowledge
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        store
            .lookup_outcome(&key(
                &store,
                &fixture.context,
                &OperationId::new(command.operation_id().as_str())?
            ))
            .await?,
        OutcomeLookup::Missing
    ));
    assert!(matches!(
        handler.handle(delivery(true)?).await,
        DeliveryDisposition::Acknowledge
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let reader =
        NatsCommandResponseReader::new(fixture.js.clone(), fixture.messaging.topology().clone());
    let before = reader
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            Duration::from_secs(1),
        )
        .await?;
    assert!(!matches!(
        handler.handle(delivery(false)?).await,
        DeliveryDisposition::Acknowledge
    ));
    assert_eq!(
        reader
            .read_command_response(
                &command.response_address()?,
                command.operation_id(),
                command.message_id(),
                Duration::from_secs(1)
            )
            .await?,
        before,
        "caller-specific denial must never overwrite the operation response"
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 3);
    assert!(
        policy
            .principals
            .lock()
            .unwrap()
            .iter()
            .all(|p| p == "verified-service")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct LargeReject;
#[async_trait]
impl CommandHandler<()> for LargeReject {
    type Rejection = String;
    async fn handle(&self, _: &(), _: &mut CommandExecution<'_>) -> CommandHandlingResult<String> {
        Ok(CommandDecision::Rejected("s".repeat(24_000)))
    }
}
fn metadata(context: &BoundedContext) -> TestResult<CommandExecutionMetadata> {
    Ok(CommandExecutionMetadata::new(
        OperationId::new("prepared-only")?,
        ContentFingerprint::digest("prepared-only"),
    )
    .with_bounded_context(context.name().clone()))
}
#[tokio::test]
async fn detail_capacity_failure_prevents_any_atomic_journal_decision() -> TestResult {
    let fixture = Fixture::new("modular-detail-capacity").await?;
    let store = split_store(&fixture).await?;
    let detail = fixture
        .js
        .get_stream(store.config().outcome_state_stream_name())
        .await?;
    let mut config = detail.cached_info().config.clone();
    config.max_bytes = 1024;
    fixture.js.update_stream(config).await?;
    assert!(
        CommandExecutor::new(store.clone())
            .execute_with_rejection_codec(
                &LargeReject,
                metadata(&fixture.context)?,
                &(),
                &JsonRejectionCodec::new("large", 1)
            )
            .await
            .is_err()
    );
    assert_eq!(
        message_count(&fixture, store.config().stream_name()).await?,
        0
    );
    assert_eq!(
        message_count(&fixture, store.config().outcome_state_stream_name()).await?,
        0
    );
    // N2: deliberately reduced capacity is configuration drift, so authoritative
    // absence is blocked until the bound pair's original configuration is restored.
    // Actual unchanged-config capacity exhaustion is covered by review_native_adversarial.
    assert_eq!(
        store
            .lookup_outcome(&key(
                &store,
                &fixture.context,
                &OperationId::new("prepared-only")?
            ))
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    fixture
        .js
        .update_stream(detail.cached_info().config.clone())
        .await?;
    assert!(matches!(
        store
            .lookup_outcome(&key(
                &store,
                &fixture.context,
                &OperationId::new("prepared-only")?
            ))
            .await?,
        OutcomeLookup::Missing
    ));
    Ok(())
}

struct Proxy(tokio::task::JoinHandle<TestResult>);
impl Drop for Proxy {
    fn drop(&mut self) {
        self.0.abort();
    }
}
/// Gate the actual final NATS atomic frame before forwarding it, after preparation has
/// necessarily been acknowledged. Dropping the proxy discards only this owned connection.
async fn gate_final_commit()
-> TestResult<(Proxy, std::net::SocketAddr, Arc<tokio::sync::Semaphore>)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = std::env::var("ROSTFREI_NATS_URL")?
        .rsplit('@')
        .next()
        .unwrap()
        .to_owned();
    let reached = Arc::new(tokio::sync::Semaphore::new(0));
    let signal = reached.clone();
    let task = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(upstream).await?;
        let (client_read, mut client_write) = client.into_split();
        let (mut upstream_read, mut upstream_write) = upstream.into_split();
        let download = async {
            tokio::io::copy(&mut upstream_read, &mut client_write).await?;
            TestResult::Ok(())
        };
        let upload = async {
            let mut reader = BufReader::new(client_read);
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).await? == 0 {
                    return TestResult::Ok(());
                }
                let text = std::str::from_utf8(&line)?;
                if text.starts_with("PUB ") || text.starts_with("HPUB ") {
                    let length: usize = text.split_whitespace().last().unwrap().parse()?;
                    let mut body = vec![0; length.checked_add(2).ok_or("frame overflow")?];
                    reader.read_exact(&mut body).await?;
                    if body
                        .windows(b"Nats-Batch-Commit:".len())
                        .any(|w| w == b"Nats-Batch-Commit:")
                    {
                        signal.add_permits(1);
                        std::future::pending::<()>().await;
                    }
                    upstream_write.write_all(&line).await?;
                    upstream_write.write_all(&body).await?;
                } else {
                    upstream_write.write_all(&line).await?;
                }
            }
        };
        tokio::select! { result = upload => result, result = download => result }
    });
    Ok((Proxy(task), address, reached))
}
#[tokio::test]
async fn acknowledged_preparation_without_cas_is_not_a_decision_and_is_retained() -> TestResult {
    let fixture = Fixture::new("modular-prepare-crash").await?;
    let store = split_store(&fixture).await?;
    let (proxy, address, reached) = gate_final_commit().await?;
    let client = async_nats::ConnectOptions::with_user_and_password(
        std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
        std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
    )
    .connect(format!("nats://{address}"))
    .await?;
    let proxied = Arc::new(
        NatsEventStore::connect(async_nats::jetstream::new(client), store.config().clone()).await?,
    );
    let input_metadata = metadata(&fixture.context)?;
    let task = tokio::spawn(async move {
        CommandExecutor::new(proxied)
            .execute_with_rejection_codec(
                &LargeReject,
                input_metadata,
                &(),
                &JsonRejectionCodec::new("large", 1),
            )
            .await
    });
    // RAII also aborts the command task on an assertion panic.
    struct Submission(
        tokio::task::JoinHandle<Result<CommandOutcome<String>, CommandExecutionError>>,
    );
    impl Drop for Submission {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let submission = Submission(task);
    tokio::time::timeout(WAIT, reached.acquire())
        .await??
        .forget();
    assert_eq!(
        message_count(&fixture, store.config().outcome_state_stream_name()).await?,
        1,
        "detail is durable before CAS was even sent"
    );
    assert_eq!(
        message_count(&fixture, store.config().stream_name()).await?,
        0
    );
    assert!(matches!(
        store
            .lookup_outcome(&key(
                &store,
                &fixture.context,
                &OperationId::new("prepared-only")?
            ))
            .await?,
        OutcomeLookup::Missing
    ));
    drop(submission);
    drop(proxy);
    let fresh =
        Arc::new(NatsEventStore::connect(fixture.js.clone(), store.config().clone()).await?);
    assert_eq!(
        message_count(&fixture, store.config().outcome_state_stream_name()).await?,
        1,
        "negative marker lookup cannot collect an unresolved preparation"
    );
    assert!(matches!(
        CommandExecutor::new(fresh)
            .execute_with_rejection_codec(
                &LargeReject,
                metadata(&fixture.context)?,
                &(),
                &JsonRejectionCodec::new("large", 1)
            )
            .await?,
        CommandOutcome::Rejected(_)
    ));
    assert_eq!(
        message_count(&fixture, store.config().stream_name()).await?,
        1
    );
    Ok(())
}

struct RestartDecision(Arc<AtomicUsize>);
#[async_trait]
impl CommandHandler<bool> for RestartDecision {
    type Rejection = String;
    async fn handle(
        &self,
        reject: &bool,
        _: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(if *reject {
            CommandDecision::Rejected("restart-secret".to_owned())
        } else {
            CommandDecision::Accepted
        })
    }
}
async fn restart_phase(mode: &str) -> TestResult {
    let fixture = Fixture::for_application(ApplicationName::new(std::env::var(
        "ROSTFREI_MODULAR_RESTART_APP",
    )?)?)
    .await?;
    let store = split_store(&fixture).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let handler = RestartDecision(calls.clone());
    for (id, reject, expire) in [
        ("expired-acceptance", false, true),
        ("expired-rejection", true, true),
        ("retained-rejection", true, false),
    ] {
        let metadata =
            CommandExecutionMetadata::new(OperationId::new(id)?, ContentFingerprint::digest(id))
                .with_bounded_context(fixture.context.name().clone());
        let outcome_key = key(&store, &fixture.context, metadata.operation_id());
        if mode == "seed" {
            CommandExecutor::new(store.clone())
                .execute_with_rejection_codec(
                    &handler,
                    metadata,
                    &reject,
                    &JsonRejectionCodec::new("restart", 1),
                )
                .await?;
            if expire {
                let mut states = fixture
                    .js
                    .get_stream(store.config().outcome_state_stream_name())
                    .await?;
                let active = modular_native_delta::capture_latest(&mut states).await?;
                let path =
                    std::path::PathBuf::from(std::env::var("ROSTFREI_MODULAR_RESTART_RECORDS")?)
                        .join(format!("{id}.json"));
                std::fs::write(path, serde_json::to_vec(&active)?)?;
                let OutcomeLookup::Completed(outcome) = store.lookup_outcome(&outcome_key).await?
                else {
                    return Err("seed completion missing".into());
                };
                let expiry = store
                    .expire_details(&outcome_key, outcome.marker().revision())
                    .await?;
                assert_eq!(expiry.reclamation(), ReclamationStatus::Reclaimed);
                modular_native_delta::assert_replaced(&states, &active).await?;
            }
        } else {
            assert_eq!(mode, "replay");
            let OutcomeLookup::Completed(outcome) = store.lookup_outcome(&outcome_key).await?
            else {
                return Err("decision lost across broker/process restart".into());
            };
            let result = CommandExecutor::new(store.clone())
                .with_max_conflict_retries(0)
                .execute_with_rejection_codec(
                    &handler,
                    metadata,
                    &reject,
                    &JsonRejectionCodec::new("restart", 1),
                )
                .await;
            if expire {
                assert!(matches!(outcome.details(), OutcomeDetails::Expired));
                assert!(result.is_err());
                let path =
                    std::path::PathBuf::from(std::env::var("ROSTFREI_MODULAR_RESTART_RECORDS")?)
                        .join(format!("{id}.json"));
                let active = serde_json::from_slice(&std::fs::read(path)?)?;
                let states = fixture
                    .js
                    .get_stream(store.config().outcome_state_stream_name())
                    .await?;
                let expired = modular_native_delta::assert_replaced(&states, &active).await?;
                modular_native_delta::assert_prepare_refused(
                    &fixture.js,
                    store.config().outcome_state_stream_name(),
                    &active,
                )
                .await?;
                let retry = store
                    .expire_details(&outcome_key, outcome.marker().revision())
                    .await?;
                assert_eq!(retry.reclamation(), ReclamationStatus::Reclaimed);
                assert_eq!(
                    modular_native_delta::assert_replaced(&states, &active).await?,
                    expired
                );
            } else {
                assert_eq!(
                    result?,
                    CommandOutcome::Rejected("restart-secret".to_owned())
                );
            }
        }
    }
    assert_eq!(
        message_count(&fixture, store.config().stream_name()).await?,
        3
    );
    assert_eq!(
        message_count(&fixture, store.config().outcome_state_stream_name()).await?,
        3,
        "one Active plus two terminal Expired records survive restart"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        if mode == "seed" { 3 } else { 0 }
    );
    Ok(())
}
#[tokio::test]
async fn split_retained_and_expired_outcomes_survive_process_and_broker_restart() -> TestResult {
    if let Ok(mode) = std::env::var("ROSTFREI_MODULAR_RESTART_MODE") {
        return restart_phase(&mode).await;
    }
    modular_restart_support::run(
        "split_retained_and_expired_outcomes_survive_process_and_broker_restart",
    )
    .await
}
