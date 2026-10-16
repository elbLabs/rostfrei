//! Authorized replay must retain current admission and truthful broker-publication evidence.
#![allow(
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod release_contract_support;

use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{
    AuthorizationError, CommandAccess, CommandAuthorizer, CommandBusErrorKind, CommandBusObserver,
    CommandProcessingService, CommandPublication, EncodedCommand, ExecutionFailure, PrincipalId,
    StoreFailureKind, TrustedCommandContext, ValidatedCommand,
};
use tokio::sync::{Mutex, Semaphore};

rostfrei::install_macro_support!();

#[derive(Default)]
struct Policy {
    denied: AtomicBool,
    accesses: std::sync::Mutex<Vec<(String, Option<ActorRef>, CommandAccess)>>,
}

#[async_trait]
impl CommandAuthorizer for Policy {
    async fn authorize(
        &self,
        caller: &TrustedCommandContext,
        command: &ValidatedCommand,
        access: CommandAccess,
    ) -> Result<(), AuthorizationError> {
        self.accesses.lock().unwrap().push((
            caller.principal().as_str().to_owned(),
            command.original().actor().cloned(),
            access,
        ));
        if self.denied.load(Ordering::SeqCst) {
            Err(AuthorizationError::Denied)
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct Publications {
    records: Mutex<Vec<CommandPublication>>,
    gate: Option<(Semaphore, Semaphore)>,
}

#[async_trait]
impl CommandBusObserver for Publications {
    async fn published(&self, publication: CommandPublication) {
        self.records.lock().await.push(publication);
        if let Some((reached, release)) = &self.gate {
            reached.add_permits(1);
            release.acquire().await.unwrap().forget();
        }
    }
}

struct Resolver(AtomicUsize);

#[async_trait]
impl TrustedCommandContextResolver for Resolver {
    async fn resolve(
        &self,
        _: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        caller("worker").map_err(|_| AuthorizationError::InvalidConfiguration)
    }
}

fn caller(name: &str) -> TestResult<TrustedCommandContext> {
    Ok(TrustedCommandContext::new(
        PrincipalId::new(name)?,
        SecurityScope::new("replay-realm")?,
    ))
}

fn actor() -> TestResult<ActorRef> {
    Ok(ActorRef::new("people", "original-actor")?)
}

struct Replay {
    fixture: Fixture,
    store: Arc<NatsEventStore>,
    service: Arc<dyn CommandProcessingService>,
    policy: Arc<Policy>,
    calls: Arc<AtomicUsize>,
    resolver: Arc<Resolver>,
    handler: Arc<NatsCommandHandler>,
    command: EncodedCommand,
}

impl Replay {
    async fn new(action: &str) -> TestResult<Self> {
        let fixture = Fixture::new("replay-publication").await?;
        let config = NatsEventStoreConfig::for_bounded_context(&fixture.context)?
            .with_fresh_split_outcome_epoch("replay-publication-v1")?
            .with_explicit_detail_expiry(true)?
            .with_storage_limits(32 * 1024 * 1024, 524_288)?;
        provision_event_store(&fixture.js, &config).await?;
        let store = Arc::new(NatsEventStore::connect(fixture.js.clone(), config).await?);
        let calls = Arc::new(AtomicUsize::new(0));
        let policy = Arc::new(Policy::default());
        let mut processor = CommandProcessor::builder(fixture.context.clone(), store.clone())
            .authorizer(policy.clone())
            .build()?;
        processor.register::<Act, _>(Handler(calls.clone()), map_rejection)?;
        let service: Arc<dyn CommandProcessingService> = Arc::new(processor);
        let resolver = Arc::new(Resolver(AtomicUsize::new(0)));
        let handler = Arc::new(
            fixture
                .adapter
                .command_handler_with_resolver(service.clone(), resolver.clone()),
        );
        let command = CommandBus::new(fixture.context.clone(), fixture.adapter.clone())
            .encode(request("original-operation", "target", action)?.with_actor(actor()?))?;
        Ok(Self {
            fixture,
            store,
            service,
            policy,
            calls,
            resolver,
            handler,
            command,
        })
    }

    fn sender(&self, publish_replay: bool) -> TestResult<CommandBus> {
        let adapter = (*self.fixture.adapter)
            .clone()
            .with_outcome_service(self.service.clone(), caller("sender")?);
        let adapter = if publish_replay {
            adapter.with_replay_publication()
        } else {
            adapter
        };
        Ok(CommandBus::new(
            self.fixture.context.clone(),
            Arc::new(adapter),
        ))
    }

    async fn seed(&self) -> TestResult<CommandResponse> {
        // The native worker persists a winner and its response before this command has
        // been published. Thus the first real PubAck must be nonduplicate even on replay.
        assert_eq!(
            self.handler.handle(command_delivery(&self.command)?).await,
            DeliveryDisposition::Acknowledge
        );
        assert_eq!(self.calls.load(Ordering::SeqCst), 1);
        self.raw_response().await
    }

    async fn raw_response(&self) -> TestResult<CommandResponse> {
        Ok(NatsCommandResponseReader::new(
            self.fixture.js.clone(),
            self.fixture.messaging.topology().clone(),
        )
        .read_command_response(
            &self.command.response_address()?,
            self.command.operation_id(),
            self.command.message_id(),
            WAIT,
        )
        .await?)
    }

    async fn expire(&self) -> TestResult {
        let OutcomeLookup::Completed(winner) = self
            .service
            .lookup(&caller("maintenance")?, &self.command)
            .await
            .into_result()?
        else {
            return Err("missing winner before expiry".into());
        };
        self.store
            .expire_details(
                winner.marker().detail_reference().key(),
                winner.marker().revision(),
            )
            .await?;
        Ok(())
    }

    async fn commands(&self) -> TestResult<async_nats::jetstream::stream::Stream> {
        Ok(self
            .fixture
            .js
            .get_stream(self.fixture.messaging.topology().command_stream().as_str())
            .await?)
    }

    async fn start_worker(&self) -> TestResult<Worker> {
        let config = ConsumerConfig::new(
            self.fixture.context.consumer_name("act", 1)?,
            self.fixture.context.durable_name("act", 1)?,
            self.command.address().clone(),
            Duration::from_secs(5),
            Duration::from_secs(1),
            2,
            1000,
        )?;
        provision_durable_consumer(&self.fixture.js, self.fixture.messaging.topology(), &config)
            .await?;
        let factory = NatsConsumerFactory::new(
            self.fixture.js.clone(),
            self.fixture.messaging.topology().clone(),
        );
        let consumer = <NatsConsumerFactory as MessageConsumerFactory<CommandAddress>>::create(
            &factory, config,
        )?;
        let handler = self.handler.clone();
        Ok(Worker(tokio::spawn(
            async move { consumer.run(handler).await },
        )))
    }

    async fn wait_acknowledged(&self) -> TestResult {
        let mut commands = self.commands().await?;
        tokio::time::timeout(WAIT, async {
            loop {
                if commands.info().await?.state.messages == 0 {
                    return TestResult::Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        assert!(self.fixture.quarantine().await?.is_empty());
        Ok(())
    }
}

#[tokio::test]
async fn default_authorized_replay_keeps_no_publication_fast_path() -> TestResult {
    for action in ["create", "add", "noop"] {
        let replay = Replay::new(action).await?;
        let winner = replay.seed().await?;
        let observer = Arc::new(Publications::default());
        let receipt = replay
            .sender(false)?
            .dispatch_encoded_observed(replay.command.clone(), observer.clone())
            .await?;
        assert_eq!(receipt.response(), &winner);
        assert!(receipt.publication_duplicate());
        assert!(observer.records.lock().await.is_empty());
        assert_eq!(
            replay.commands().await?.cached_info().state.last_sequence,
            0
        );
        assert_eq!(replay.calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn opt_in_replay_publishes_original_command_and_reconciles_durable_decisions() -> TestResult {
    for action in ["create", "add", "noop"] {
        let replay = Replay::new(action).await?;
        let winner = replay.seed().await?;
        assert_eq!(rejected(winner.outcome()), action == "add");
        let before = replay
            .service
            .lookup(&caller("sender")?, &replay.command)
            .await
            .into_result()?;
        let history = replay.store.load(&stream("target")?).await?;
        assert_eq!(history.len(), usize::from(action == "create"));
        replay.policy.accesses.lock().unwrap().clear();
        let bus = replay.sender(true)?;
        let observer = Arc::new(Publications::default());
        let first = bus
            .dispatch_encoded_observed(replay.command.clone(), observer.clone())
            .await?;
        assert_eq!(first.response(), &winner);
        assert_eq!(
            observer.records.lock().await.as_slice(),
            &[CommandPublication::new(
                replay.command.message_id().clone(),
                false
            )],
            "opt-in replay requires an actual publication callback, with the broker's duplicate flag"
        );
        assert!(
            !first.publication_duplicate(),
            "a durable winner is not a duplicate PubAck"
        );
        let mut commands = replay.commands().await?;
        let state = commands.info().await?.state.clone();
        assert_eq!(state.messages, 1);
        let raw = commands.get_raw_message(state.last_sequence).await?;
        assert_eq!(raw.payload.as_ref(), replay.command.payload());
        assert_eq!(
            raw.headers
                .get("Nats-Msg-Id")
                .map(async_nats::HeaderValue::as_str),
            Some(replay.command.message_id().as_str())
        );
        let duplicate = bus
            .dispatch_encoded_observed(replay.command.clone(), observer.clone())
            .await?;
        assert_eq!(duplicate.response(), &winner);
        assert!(duplicate.publication_duplicate());
        assert_eq!(
            observer.records.lock().await.as_slice(),
            &[
                CommandPublication::new(replay.command.message_id().clone(), false),
                CommandPublication::new(replay.command.message_id().clone(), true),
            ]
        );
        // An independent actual PubAck confirms the stream/sequence and duplicate identity.
        let ack = replay
            .fixture
            .publisher()
            .publish_command_with_ack(replay.command.message().clone(), WAIT)
            .await?;
        assert!(ack.duplicate());
        assert_eq!(
            ack.stream(),
            replay.fixture.messaging.topology().command_stream()
        );
        assert_eq!(ack.sequence(), state.last_sequence);
        let _worker = replay.start_worker().await?;
        replay.wait_acknowledged().await?;
        assert_eq!(replay.resolver.0.load(Ordering::SeqCst), 2);
        assert_eq!(replay.calls.load(Ordering::SeqCst), 1);
        assert_eq!(replay.raw_response().await?, winner);
        assert_eq!(replay.store.load(&stream("target")?).await?, history);
        assert_eq!(
            replay
                .service
                .lookup(&caller("sender")?, &replay.command)
                .await
                .into_result()?,
            before
        );
        let accesses = replay.policy.accesses.lock().unwrap();
        assert!(accesses.iter().all(|(_, claim, access)| claim.as_ref()
            == Some(&actor().unwrap())
            && *access == CommandAccess::ReadOutcome));
        assert!(
            accesses
                .iter()
                .any(|(principal, _, _)| principal == "worker")
        );
        drop(accesses);
    }
    Ok(())
}

#[tokio::test]
async fn opt_in_fresh_commands_still_execute_once_through_native_worker() -> TestResult {
    for action in ["create", "add", "noop"] {
        let replay = Replay::new(action).await?;
        let _worker = replay.start_worker().await?;
        let observer = Arc::new(Publications::default());
        let receipt = replay
            .sender(true)?
            .dispatch_encoded_observed(replay.command.clone(), observer.clone())
            .await?;
        assert_eq!(rejected(receipt.response().outcome()), action == "add");
        assert!(!receipt.publication_duplicate());
        assert_eq!(
            observer.records.lock().await.as_slice(),
            &[CommandPublication::new(
                replay.command.message_id().clone(),
                false
            ),]
        );
        replay.wait_acknowledged().await?;
        assert_eq!(replay.calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn opt_in_preserves_execution_failed_mapping_without_a_business_winner() -> TestResult {
    let replay = Replay::new("create").await?;
    let failure = ExecutionFailure::Store(StoreFailureKind::CorruptHistory);
    let response = CommandResponse::execution_failed(
        rostfrei::command_technical_response_message_id(replay.command.message_id(), failure)?,
        replay.command.message_id().clone(),
        replay.command.address().clone(),
        replay.command.operation_id().clone(),
        replay.command.correlation_id().clone(),
        failure,
    )?;
    replay
        .fixture
        .publisher()
        .publish_command_response(OutboundMessage::json(
            replay.command.response_address()?,
            response.message_id().clone(),
            &response,
        )?)
        .await?;
    let observer = Arc::new(Publications::default());
    let error = replay
        .sender(true)?
        .dispatch_encoded_observed(replay.command.clone(), observer.clone())
        .await
        .unwrap_err();
    assert_eq!(error.execution_failure(), Some(failure));
    assert_eq!(
        observer.records.lock().await.as_slice(),
        &[CommandPublication::new(
            replay.command.message_id().clone(),
            false
        ),]
    );
    assert_eq!(replay.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn opt_in_revoke_and_expiry_block_before_publication_with_retained_response() -> TestResult {
    for action in ["create", "add", "noop"] {
        let replay = Replay::new(action).await?;
        let winner = replay.seed().await?;
        let bus = replay.sender(true)?;
        let observer = Arc::new(Publications::default());
        replay.policy.denied.store(true, Ordering::SeqCst);
        assert_eq!(
            bus.dispatch_encoded_observed(replay.command.clone(), observer.clone())
                .await
                .unwrap_err()
                .authorization_error(),
            Some(&AuthorizationError::Denied)
        );
        replay.policy.denied.store(false, Ordering::SeqCst);
        replay.expire().await?;
        assert_eq!(
            bus.dispatch_encoded_observed(replay.command.clone(), observer.clone())
                .await
                .unwrap_err()
                .kind(),
            CommandBusErrorKind::CompletedDetailsExpired
        );
        assert!(observer.records.lock().await.is_empty());
        assert_eq!(
            replay.commands().await?.cached_info().state.last_sequence,
            0
        );
        assert_eq!(replay.raw_response().await?, winner);
        assert_eq!(replay.calls.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[tokio::test]
async fn opt_in_replay_still_refuses_changed_actor_and_security_scope_before_publication()
-> TestResult {
    let replay = Replay::new("create").await?;
    let winner = replay.seed().await?;
    let bus = replay.sender(true)?;
    let observer = Arc::new(Publications::default());
    for request in [
        request("original-operation", "target", "create")?,
        request("original-operation", "target", "create")?
            .with_actor(ActorRef::new("people", "changed-actor")?),
    ] {
        let changed = bus.encode(request)?;
        assert_eq!(changed.operation_id(), replay.command.operation_id());
        assert_eq!(
            bus.dispatch_encoded_observed(changed, observer.clone())
                .await
                .unwrap_err()
                .kind(),
            CommandBusErrorKind::InvalidMessage
        );
    }
    let wrong_scope = TrustedCommandContext::new(
        PrincipalId::new("sender")?,
        SecurityScope::new("another-realm")?,
    );
    let adapter = (*replay.fixture.adapter)
        .clone()
        .with_replay_publication()
        .with_outcome_service(replay.service.clone(), wrong_scope);
    let wrong_scope_bus = CommandBus::new(replay.fixture.context.clone(), Arc::new(adapter));
    assert!(
        wrong_scope_bus
            .dispatch_encoded_observed(replay.command.clone(), observer.clone())
            .await
            .is_err()
    );
    assert!(observer.records.lock().await.is_empty());
    assert_eq!(
        replay.commands().await?.cached_info().state.last_sequence,
        0
    );
    assert_eq!(replay.raw_response().await?, winner);
    assert_eq!(replay.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn opt_in_rechecks_current_admission_after_puback_with_retained_response() -> TestResult {
    for action in ["create", "add", "noop"] {
        for expire in [false, true] {
            let replay = Replay::new(action).await?;
            let winner = replay.seed().await?;
            let bus = replay.sender(true)?;
            let observer = Arc::new(Publications {
                records: Mutex::default(),
                gate: Some((Semaphore::new(0), Semaphore::new(0))),
            });
            let dispatch = bus.dispatch_encoded_observed(replay.command.clone(), observer.clone());
            let change = async {
                let (reached, release) = observer.gate.as_ref().unwrap();
                reached.acquire().await?.forget();
                if expire {
                    replay.expire().await?;
                } else {
                    replay.policy.denied.store(true, Ordering::SeqCst);
                }
                release.add_permits(1);
                TestResult::Ok(())
            };
            let (result, changed) =
                tokio::time::timeout(WAIT, async { tokio::join!(dispatch, change) }).await?;
            changed?;
            let error = result.unwrap_err();
            if expire {
                assert_eq!(error.kind(), CommandBusErrorKind::CompletedDetailsExpired);
            } else {
                assert_eq!(
                    error.authorization_error(),
                    Some(&AuthorizationError::Denied)
                );
            }
            assert_eq!(
                observer.records.lock().await.as_slice(),
                &[CommandPublication::new(
                    replay.command.message_id().clone(),
                    false
                ),]
            );
            assert_eq!(
                replay.commands().await?.cached_info().state.last_sequence,
                1
            );
            assert_eq!(replay.raw_response().await?, winner);
            assert_eq!(replay.calls.load(Ordering::SeqCst), 1);
        }
    }
    Ok(())
}
