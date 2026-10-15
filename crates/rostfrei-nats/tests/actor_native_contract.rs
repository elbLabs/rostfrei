//! ACT-14..16. Run only `actor_` tests: source import retains unrelated original unit tests.
//! Future ActorRef API; real owned NATS, no production changes or replacement store.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::panic_in_result_fn,
    clippy::indexing_slicing
)]
mod modular_restart_support;
mod release_contract_support;
// Source import gives test-only access to existing serialization boundaries.
#[path = "../src/event_store_config.rs"]
mod event_store_config;
#[path = "../src/hex.rs"]
mod hex;
#[path = "../src/stream_policy.rs"]
mod stream_policy;
use event_store_config::NatsEventStoreConfig;
mod event_store {
    include!("../src/event_store.rs");
    pub fn actor_check_receipt(
        config: &NatsEventStoreConfig,
        sequence: u64,
        headers: &HeaderMap,
        bytes: &[u8],
    ) -> Result<(), EventStoreError> {
        decode_transaction_receipt(config, sequence, headers, bytes).map(|_| ())
    }
}
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{
    ActorRef, AllowAllCommands, AuthorizationError, CommandAccess, CommandAuthorizer,
    CommandProcessingService, CompletionOrigin, PrincipalId, TrustedCommandContext,
    ValidatedCommand,
};
use std::sync::Mutex;
rostfrei::install_macro_support!();

fn actor(id: &str) -> ActorRef {
    ActorRef::new("people", id).unwrap()
}
fn caller(id: &str) -> TrustedCommandContext {
    TrustedCommandContext::new(
        PrincipalId::new(id).unwrap(),
        SecurityScope::new("actor-test-scope").unwrap(),
    )
}
async fn store(f: &Fixture) -> TestResult<Arc<NatsEventStore>> {
    let config = rostfrei_nats::NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("actor-contract")?
        .with_explicit_detail_expiry(true)?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?;
    rostfrei_nats::provision_event_store(&f.js, &config).await?;
    Ok(Arc::new(
        NatsEventStore::connect(f.js.clone(), config).await?,
    ))
}
fn service(
    f: &Fixture,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
    policy: Arc<dyn CommandAuthorizer>,
) -> TestResult<Arc<dyn CommandProcessingService>> {
    let mut p = CommandProcessor::builder(f.context.clone(), store)
        .authorizer(policy)
        .build()?;
    p.register::<Act, _>(Handler(calls), map_rejection)?;
    Ok(Arc::new(p))
}
#[derive(Default)]
struct Policy(Mutex<Vec<(String, Option<ActorRef>)>>);
#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        context: &TrustedCommandContext,
        command: &ValidatedCommand,
        _: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.0.lock().unwrap().push((
            context.principal().as_str().to_owned(),
            command.original().actor().cloned(),
        ));
        if context.principal().as_str() != "verified-worker"
            || command.original().actor() != Some(&actor("alice"))
        {
            return Err(AuthorizationError::Denied);
        }
        Ok(())
    }
}
struct Resolver;
#[async_trait]
impl TrustedCommandContextResolver for Resolver {
    async fn resolve(
        &self,
        delivery: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError> {
        // Fixture evidence checked independently of actor/principal claims; no signature protocol.
        if delivery.metadata().get("fixture-proof") != Some("trusted-producer-channel") {
            return Err(AuthorizationError::Unauthenticated);
        }
        Ok(caller("verified-worker"))
    }
}
#[tokio::test]
async fn actor_native_resolver_keeps_authenticated_principal_distinct_on_replay() -> TestResult {
    let f = Fixture::new("actor-resolver").await?;
    let store = store(&f).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let policy = Arc::new(Policy::default());
    let service = service(&f, store.clone(), calls.clone(), policy.clone())?;
    let worker = f
        .adapter
        .command_handler_with_resolver(service, Arc::new(Resolver));
    let bus = CommandBus::new(f.context.clone(), f.adapter.clone());
    let original =
        bus.encode(request("actor-worker", "one", "create")?.with_actor(actor("alice")))?;
    let forged =
        bus.encode(request("actor-forged", "two", "create")?.with_actor(actor("admin")))?;
    let delivery = |command: &rostfrei::EncodedCommand,
                    proof: bool|
     -> TestResult<MessageDelivery<CommandAddress>> {
        let mut metadata = CallerMetadata::try_from_entries([("principal", "admin")])?;
        if proof {
            metadata.insert("fixture-proof", "trusted-producer-channel")?;
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
        worker.handle(delivery(&original, false)?).await,
        DeliveryDisposition::Acknowledge
    ));
    assert!(!matches!(
        worker.handle(delivery(&forged, true)?).await,
        DeliveryDisposition::Acknowledge
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for _ in 0..2 {
        assert!(matches!(
            worker.handle(delivery(&original, true)?).await,
            DeliveryDisposition::Acknowledge
        ));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.load(&stream("one")?).await?[0].actor(),
        Some(&actor("alice"))
    );
    assert!(store.load(&stream("two")?).await?.is_empty());
    let rows = policy.0.lock().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.0 == "verified-worker"));
    assert!(rows.iter().any(|row| row.1 == Some(actor("admin"))));
    Ok(())
}

#[tokio::test]
async fn actor_native_event_and_receipt_wire_mutations_fail_integrity_checks() -> TestResult {
    let f = Fixture::new("actor-wire").await?;
    let store = store(&f).await?;
    let service = service(
        &f,
        store.clone(),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AllowAllCommands),
    )?;
    let command = CommandBus::new(f.context.clone(), f.adapter.clone())
        .encode(request("actor-wire", "one", "create")?.with_actor(actor("alice")))?;
    service
        .process(&caller("worker"), &command)
        .await
        .into_result()?;
    let journal = f.js.get_stream(store.config().stream_name()).await?;
    let event = journal
        .get_raw_message(journal.cached_info().state.first_sequence)
        .await?;
    let receipt = journal
        .get_raw_message(journal.cached_info().state.last_sequence)
        .await?;
    let raw_stream = stream("standalone")?;
    let raw_operation = OperationId::new("standalone")?;
    let raw_commit = derive_commit_id(&raw_stream, &raw_operation);
    store
        .append(
            &raw_stream,
            ExpectedVersion::NoStream,
            EventBatch::new(
                raw_commit.clone(),
                raw_operation,
                ContentFingerprint::digest("standalone"),
                vec![NewEvent::new(
                    derive_event_id(&raw_commit, 0),
                    "changed",
                    1,
                    br#"{"created":true,"item":null}"#.to_vec(),
                )?],
            )?
            .with_actor(actor("alice")),
        )
        .await?;
    let journal = f.js.get_stream(store.config().stream_name()).await?;
    let standalone = journal
        .get_raw_message(journal.cached_info().state.last_sequence)
        .await?;
    let config = NatsEventStoreConfig::for_bounded_context(&f.context)?
        .with_fresh_split_outcome_epoch("actor-contract")?
        .with_explicit_detail_expiry(true)?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?;
    for (field, raw, expected_schema) in [
        ("event", &event, 6),
        ("event", &standalone, 5),
        ("receipt", &receipt, 5),
    ] {
        let check = |bytes: &[u8]| {
            if field == "event" {
                event_store::decode_consumed_event(
                    &config,
                    raw.subject.as_str(),
                    &raw.headers,
                    bytes,
                )
                .map(|_| ())
            } else {
                event_store::actor_check_receipt(&config, raw.sequence, &raw.headers, bytes)
            }
        };
        check(&raw.payload)?;
        let wire: serde_json::Value = serde_json::from_slice(&raw.payload)?;
        assert_eq!(wire["schemaVersion"], expected_schema);
        assert_eq!(wire[field]["actor"], serde_json::to_value(actor("alice"))?);
        for mutation in ["replace", "strip", "null", "legacy-version"] {
            let mut bad = wire.clone();
            match mutation {
                "replace" => bad[field]["actor"] = serde_json::to_value(actor("bob"))?,
                "strip" => {
                    bad[field].as_object_mut().unwrap().remove("actor");
                }
                "null" => bad[field]["actor"] = serde_json::Value::Null,
                _ => bad["schemaVersion"] = serde_json::json!(4),
            }
            let bytes = serde_json::to_vec(&bad)?;
            let result = check(&bytes);
            assert_eq!(
                result.unwrap_err().kind(),
                EventStoreErrorKind::CorruptHistory,
                "{field}: {mutation}"
            );
        }
    }
    Ok(())
}

struct PrivateActor(tokio::sync::mpsc::Sender<Option<ActorRef>>);
#[async_trait]
impl DomainEventHandler<Changed> for PrivateActor {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, Changed>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0
            .send(event.recorded().actor().cloned())
            .await
            .map_err(|e| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    e.to_string(),
                )
            })
    }
}
async fn private_delivery_after_expiry(f: &Fixture, store: &NatsEventStore) -> TestResult {
    let config = NatsDomainEventConsumerConfig::new(
        f.context.consumer_name("actor-private", 1)?,
        f.context.durable_name("actor-private", 1)?,
        Duration::from_secs(5),
        Duration::from_secs(2),
        RetryDelay::new(Duration::from_millis(25))?,
    )?;
    provision_domain_event_consumer(&f.js, store.config(), &config).await?;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register::<Target, Changed, _>("changed", Arc::new(PrivateActor(sender)))?;
    let consumer = NatsDomainEventConsumer::connect(
        f.js.clone(),
        store.config().clone(),
        config,
        Arc::new(dispatcher),
    )
    .await?;
    let (shutdown, shutdown_rx) = tokio::sync::watch::channel(false);
    let running = consumer.run_until_shutdown(shutdown_rx);
    tokio::pin!(running);
    tokio::time::timeout(WAIT, async {
        for _ in 0..2 {
            tokio::select! {
                value = receiver.recv() => assert_eq!(value, Some(Some(actor("alice")))),
                result = &mut running => return Err(format!("consumer ended: {result:?}").into()),
            }
        }
        shutdown.send(true)?;
        running.await?;
        TestResult::Ok(())
    })
    .await??;
    Ok(())
}
async fn restart_phase(mode: &str) -> TestResult {
    let f = Fixture::for_application(ApplicationName::new(std::env::var(
        "ROSTFREI_MODULAR_RESTART_APP",
    )?)?)
    .await?;
    let store = store(&f).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let service = service(&f, store.clone(), calls.clone(), Arc::new(AllowAllCommands))?;
    let directory = std::path::PathBuf::from(std::env::var("ROSTFREI_MODULAR_RESTART_RECORDS")?);
    for action in ["create", "noop", "add"] {
        for expired in [false, true] {
            let id = format!("{action}-{expired}");
            let command = CommandBus::new(f.context.clone(), f.adapter.clone())
                .encode(request(&id, &id, action)?.with_actor(actor("alice")))?;
            let path = directory.join(format!("actor-{id}.json"));
            if mode == "seed" {
                let first = service
                    .process(&caller("original-worker"), &command)
                    .await
                    .into_result()?;
                let marker = first.outcome().marker();
                assert_eq!(marker.actor(), Some(&actor("alice")));
                std::fs::write(&path, marker.encode_manifest()?)?;
                if expired {
                    assert_eq!(
                        store
                            .expire_details(marker.detail_reference().key(), marker.revision())
                            .await?
                            .reclamation(),
                        ReclamationStatus::Reclaimed
                    );
                }
            } else {
                assert_eq!(mode, "replay");
                let OutcomeLookup::Completed(outcome) = service
                    .lookup(&caller("different-worker"), &command)
                    .await
                    .into_result()?
                else {
                    return Err("restart lost completion".into());
                };
                assert_eq!(outcome.marker().actor(), Some(&actor("alice")));
                assert_eq!(outcome.marker().encode_manifest()?, std::fs::read(path)?);
                assert_eq!(
                    matches!(outcome.details(), OutcomeDetails::Expired),
                    expired
                );
                let replay = service
                    .process(&caller("different-worker"), &command)
                    .await
                    .into_result()?;
                assert_eq!(replay.origin(), CompletionOrigin::Replayed);
                assert_eq!(replay.outcome(), &outcome);
                let changed = CommandBus::new(f.context.clone(), f.adapter.clone())
                    .encode(request(&id, &id, action)?.with_actor(actor("bob")))?;
                assert_eq!(
                    service
                        .process(&caller("different-worker"), &changed)
                        .await
                        .into_result()
                        .unwrap_err()
                        .kind(),
                    rostfrei::CommandProcessorErrorKind::IdentityConflict
                );
                let history = store.load(&stream(&id)?).await?;
                assert_eq!(history.len(), usize::from(action == "create"));
                assert_eq!(store.audit_history(&stream(&id)?).await?, history);
                for event in &history {
                    assert_eq!(event.actor(), Some(&actor("alice")));
                }
                let state = CommandExecutor::new(store.clone())
                    .rehydrate::<Target>(&stream(&id)?)
                    .await?;
                assert_eq!(state.state().exists, action == "create");
            }
        }
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        if mode == "seed" { 6 } else { 0 }
    );
    if mode == "replay" {
        private_delivery_after_expiry(&f, &store).await?;
    }
    Ok(())
}
#[tokio::test]
async fn actor_native_original_attribution_survives_restart_expiry_and_private_delivery()
-> TestResult {
    if let Ok(mode) = std::env::var("ROSTFREI_MODULAR_RESTART_MODE") {
        return restart_phase(&mode).await;
    }
    modular_restart_support::run(
        "actor_native_original_attribution_survives_restart_expiry_and_private_delivery",
    )
    .await
}
