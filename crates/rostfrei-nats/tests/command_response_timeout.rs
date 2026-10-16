//! Bounded sender waiting does not reject, cancel or redrive durable work.
#![allow(clippy::panic_in_result_fn, reason = "integration contract assertions")]
mod release_contract_support;

use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{
    AllowAllCommands, AuthorizationError, CommandBusErrorKind, CommandProcessingService,
    PrincipalId, TrustedCommandContext,
};
use tokio::{
    sync::Notify,
    time::{Instant, sleep, timeout},
};
rostfrei::install_macro_support!();

fn adapter(f: &Fixture) -> NatsMessagingAdapter {
    NatsMessagingAdapter::new(
        f.publisher(),
        NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone()),
    )
}

async fn worker(f: &Fixture, handler: NatsCommandHandler) -> TestResult<Worker> {
    let config = ConsumerConfig::new(
        f.context.consumer_name("act", 1)?,
        f.context.durable_name("act", 1)?,
        f.context.command_address("act")?,
        Duration::from_secs(30),
        Duration::from_secs(20),
        1,
        3,
    )?;
    provision_durable_consumer(&f.js, f.messaging.topology(), &config).await?;
    let factory = NatsConsumerFactory::new(f.js.clone(), f.messaging.topology().clone());
    let consumer = factory.create(config)?;
    Ok(Worker(tokio::spawn(async move {
        consumer.run(Arc::new(handler)).await
    })))
}

async fn acknowledged(f: &Fixture) -> TestResult {
    let stream =
        f.js.get_stream(f.messaging.topology().command_stream().as_str())
            .await?;
    let mut consumer: async_nats::jetstream::consumer::PullConsumer = stream
        .get_consumer(f.context.durable_name("act", 1)?.as_str())
        .await?;
    timeout(WAIT, async {
        loop {
            let info = consumer.info().await?;
            if info.num_pending == 0 && info.num_ack_pending == 0 {
                return TestResult::Ok(());
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await?
}

#[tokio::test]
async fn default_wait_is_thirty_seconds_without_a_command_worker() -> TestResult {
    let f = Fixture::new("default-response-timeout").await?;
    // Deliberately use the public constructor without a timeout override.
    let bus = CommandBus::new(f.context.clone(), Arc::new(adapter(&f)));
    let command = bus.encode(request("no-worker", "target", "create")?)?;
    let start = Instant::now();
    let result = timeout(
        Duration::from_secs(45),
        bus.dispatch_encoded(command.clone()),
    )
    .await?;
    let Err(error) = result else {
        return Err("missing worker unexpectedly completed the command".into());
    };
    assert_eq!(error.kind(), CommandBusErrorKind::Timeout);
    assert!(error.execution_failure().is_none());
    assert!(start.elapsed() >= Duration::from_secs(30));
    let commands =
        f.js.get_stream(f.messaging.topology().command_stream().as_str())
            .await?;
    assert_eq!(commands.cached_info().state.messages, 1);
    let stored = commands.get_raw_message(1).await?;
    assert_eq!(stored.payload.as_ref(), command.payload());
    assert_eq!(
        f.messages(f.messaging.topology().command_response_stream().as_str())
            .await?,
        0
    );
    assert!(f.quarantine().await?.is_empty());
    Ok(())
}

struct RefuseContext;
#[async_trait]
impl TrustedCommandContextResolver for RefuseContext {
    async fn resolve(
        &self,
        _: &MessageDelivery<CommandAddress>,
    ) -> Result<TrustedCommandContext, AuthorizationError> {
        Err(AuthorizationError::Unauthenticated)
    }
}

#[tokio::test]
async fn quarantined_command_without_response_returns_only_a_timeout() -> TestResult {
    let f = Fixture::new("quarantined-response-timeout").await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let service = processor(
        &f.context,
        Arc::new(InMemoryEventStore::new()),
        calls.clone(),
        None,
    )?;
    let sender = adapter(&f).with_response_timeout(Duration::from_secs(1));
    let handler = sender.command_handler_with_resolver(service, Arc::new(RefuseContext));
    let _worker = worker(&f, handler).await?;
    let bus = CommandBus::new(f.context.clone(), Arc::new(sender));
    let command = bus.encode(request("quarantined", "target", "create")?)?;
    let result = timeout(WAIT, bus.dispatch_encoded(command.clone())).await?;
    let Err(error) = result else {
        return Err("quarantined command unexpectedly returned an outcome".into());
    };
    assert_eq!(error.kind(), CommandBusErrorKind::Timeout);
    assert!(error.execution_failure().is_none());
    acknowledged(&f).await?;
    let records = f.quarantine().await?;
    assert_eq!(records.len(), 1);
    let record = records.first().ok_or("missing quarantine record")?;
    assert_eq!(record.message_id(), command.message_id().as_str());
    assert_eq!(
        record.reason(),
        "trusted command context resolution refused delivery"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        f.messages(f.messaging.topology().command_response_stream().as_str())
            .await?,
        0
    );
    Ok(())
}

#[derive(Default)]
struct Gate {
    entered: Notify,
    release: Notify,
    calls: AtomicUsize,
}

struct GatedHandler(Arc<Gate>);
#[async_trait]
impl CommandHandler<Act> for GatedHandler {
    type Rejection = &'static str;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        self.0.calls.fetch_add(1, Ordering::SeqCst);
        self.0.entered.notify_one();
        self.0.release.notified().await;
        if command.action == "reject" {
            return Ok(CommandDecision::Rejected("target-not-found"));
        }
        let mut target = execution.load::<Target>(&command.target).await?;
        target.aggregate_mut().raise(Changed {
            created: true,
            item: None,
        });
        Ok(CommandDecision::Accepted)
    }
}

async fn late_completion(action: &str) -> TestResult {
    let f = Fixture::new("late-response-timeout").await?;
    let store = f.split_event_store().await?;
    let gate = Arc::new(Gate::default());
    let mut processor = CommandProcessor::builder(f.context.clone(), store.clone())
        .authorizer(Arc::new(AllowAllCommands))
        .conflict_retry_policy(Arc::new(NoConflictRetries))
        .build()?;
    processor.register::<Act, _>(GatedHandler(gate.clone()), map_rejection)?;
    let service: Arc<dyn CommandProcessingService> = Arc::new(processor);
    let caller = TrustedCommandContext::new(
        PrincipalId::new("response-timeout")?,
        SecurityScope::new("response-timeout")?,
    );
    let sender = adapter(&f)
        .with_response_timeout(Duration::from_millis(200))
        .with_outcome_service(service.clone(), caller.clone());
    let handler =
        sender.command_handler_with_resolver(service, Arc::new(ServicePrincipalContext(caller)));
    let _worker = worker(&f, handler).await?;
    let bus = CommandBus::new(f.context.clone(), Arc::new(sender));
    let command = bus.encode(request("late-completion", "target", action)?)?;
    // Release only after the caller timed out AND the real worker entered the handler.
    let (result, ()) = timeout(WAIT, async {
        tokio::join!(
            bus.dispatch_encoded(command.clone()),
            gate.entered.notified()
        )
    })
    .await?;
    let Err(error) = result else {
        return Err("blocked handler unexpectedly returned an outcome".into());
    };
    assert_eq!(error.kind(), CommandBusErrorKind::Timeout);
    assert!(error.execution_failure().is_none());
    assert!(store.load(&stream("target")?).await?.is_empty());
    gate.release.notify_one();

    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let completed = reader
        .read_command_response(
            &command.response_address()?,
            command.operation_id(),
            command.message_id(),
            WAIT,
        )
        .await?;
    acknowledged(&f).await?;
    let journal_messages = f.messages(store.config().stream_name()).await?;
    let replay = timeout(WAIT, bus.dispatch_encoded(command)).await??;
    assert_eq!(replay.response(), &completed);
    assert_eq!(rejected(completed.outcome()), action == "reject");
    if action != "reject" {
        assert_eq!(completed.outcome(), &CommandResponseOutcome::Accepted);
    }
    assert_eq!(gate.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.load(&stream("target")?).await?.len(),
        usize::from(action != "reject")
    );
    assert_eq!(
        f.messages(store.config().stream_name()).await?,
        journal_messages
    );
    let commands =
        f.js.get_stream(f.messaging.topology().command_stream().as_str())
            .await?;
    // Work-queue retention removes the acknowledged source; replay publishes no new one.
    assert_eq!(commands.cached_info().state.messages, 0);
    assert_eq!(commands.cached_info().state.last_sequence, 1);
    assert_eq!(
        f.messages(f.messaging.topology().command_response_stream().as_str())
            .await?,
        1
    );
    assert!(f.quarantine().await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn timeout_does_not_cancel_late_acceptance_or_reexecute_on_retry() -> TestResult {
    Box::pin(late_completion("create")).await
}

#[tokio::test]
async fn timeout_does_not_replace_late_rejection_or_reexecute_on_retry() -> TestResult {
    Box::pin(late_completion("reject")).await
}
