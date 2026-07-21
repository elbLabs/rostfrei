//! Authenticated command delivery, durable execution/response replay, and post-commit delivery.
#![allow(
    clippy::panic_in_result_fn,
    reason = "integration assertions report test failures"
)]

use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use rostfrei::{
    CommandBus, CommandExecutor, CommandProcessor, CommandRequest, InfallibleCommandRejectionMapper,
};
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, CommandDecision, CommandExecution, CommandHandler,
    CommandHandlingResult, CommittedDomainEvent, DomainEventDispatcher, DomainEventHandler,
    DomainEventHandlerError, DomainEventHandlerErrorKind, EventStore, OperationId, RecordedEvent,
    StreamId,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CausationId, CommandResponseOutcome, ConsumerConfig,
    MessageConsumerFactory, RetryDelay,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsConnection, NatsDomainEventConsumer,
    NatsDomainEventConsumerConfig, NatsEventStore, NatsEventStoreConfig, connect,
    provision_application_messaging, provision_domain_event_consumer, provision_durable_consumer,
    provision_event_store,
};
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
    time::timeout,
};

use super::{Server, TestResult, WAIT, unique_name};

#[derive(rostfrei::BoundedContext)]
#[domain(id = "authenticated-counter", label = "Authenticated counter")]
struct CounterContext;

#[derive(Clone, Debug, Eq, PartialEq, rostfrei::Command)]
#[domain(context = CounterContext, id = "increment", label = "Increment")]
struct Increment {
    counter_id: String,
    amount: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[domain(id = "incremented", label = "Incremented")]
struct Incremented {
    amount: u64,
}

#[derive(rostfrei::AggregateEvents)]
enum CounterEvents {
    Incremented(Incremented),
}

struct Counter;

impl Aggregate for Counter {
    type State = u64;
    type Event = CounterEvents;

    const BOUNDED_CONTEXT: &'static str = "authenticated-counter";
    const AGGREGATE_TYPE: &'static str = "counter";

    fn initial(_stream_id: &StreamId) -> u64 {
        0
    }

    fn apply(state: &mut u64, event: &CounterEvents) {
        let CounterEvents::Incremented(event) = event;
        *state = state.saturating_add(event.amount);
    }
}

struct IncrementHandler(Arc<AtomicUsize>);

#[async_trait]
impl CommandHandler<Increment> for IncrementHandler {
    type Rejection = Infallible;

    async fn handle(
        &self,
        command: &Increment,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Infallible> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let mut counter = execution.load::<Counter>(&command.counter_id).await?;
        counter.raise(Incremented {
            amount: command.amount,
        });
        Ok(CommandDecision::Accepted)
    }
}

struct RecordCommitted(mpsc::Sender<(Incremented, RecordedEvent)>);

#[async_trait]
impl DomainEventHandler<Incremented> for RecordCommitted {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, Incremented>,
    ) -> Result<(), DomainEventHandlerError> {
        self.0
            .send((event.event().clone(), event.recorded().clone()))
            .await
            .map_err(|_| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    "authenticated test receiver closed",
                )
            })
    }
}

fn processor(
    context: &BoundedContext,
    store: &NatsEventStore,
    calls: &Arc<AtomicUsize>,
) -> TestResult<CommandProcessor> {
    let mut processor = CommandProcessor::new(context.name().clone(), Arc::new(store.clone()));
    processor.register::<Increment, _>(
        IncrementHandler(calls.clone()),
        InfallibleCommandRejectionMapper,
    )?;
    Ok(processor)
}

async fn wait_for_ack(
    connection: &NatsConnection,
    stream_name: &str,
    durable_name: &str,
) -> TestResult<()> {
    timeout(WAIT, async {
        let stream = connection.jetstream().get_stream(stream_name).await?;
        let mut consumer = stream
            .get_consumer::<async_nats::jetstream::consumer::pull::Config>(durable_name)
            .await?;
        loop {
            let info = consumer.info().await?;
            if info.ack_floor.stream_sequence > 0
                && info.num_ack_pending == 0
                && info.num_pending == 0
            {
                return TestResult::Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?
}

async fn consume_committed_history(
    connection: &NatsConnection,
    context: &BoundedContext,
    store_config: &NatsEventStoreConfig,
    expected: &RecordedEvent,
) -> TestResult<()> {
    let config = NatsDomainEventConsumerConfig::new(
        context.consumer_name("post-commit", 1)?,
        context.durable_name("post-commit", 1)?,
        Duration::from_secs(10),
        Duration::from_secs(5),
        RetryDelay::new(Duration::from_millis(50))?,
    )?;
    provision_domain_event_consumer(connection.jetstream(), store_config, &config).await?;
    let (sender, mut receiver) = mpsc::channel(4);
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher
        .register::<Counter, Incremented, _>("incremented", Arc::new(RecordCommitted(sender)))?;
    let consumer = NatsDomainEventConsumer::connect(
        connection.jetstream().clone(),
        store_config.clone(),
        config.clone(),
        Arc::new(dispatcher),
    )
    .await?;
    let (shutdown, shutdown_rx) = watch::channel(false);
    // JoinSet also aborts the worker if an assertion, timeout, or API call fails.
    let mut workers = JoinSet::new();
    workers.spawn(consumer.run_until_shutdown(shutdown_rx));
    let observed = tokio::select! {
        result = timeout(WAIT, receiver.recv()) => result?.ok_or("post-commit receiver closed")?,
        result = workers.join_next() => return Err(format!("post-commit worker stopped: {result:?}").into()),
    };
    assert_eq!(observed.0, Incremented { amount: 7 });
    assert_eq!(&observed.1, expected);
    wait_for_ack(
        connection,
        store_config.stream_name(),
        config.durable_name().as_str(),
    )
    .await?;
    shutdown.send(true)?;
    timeout(WAIT, workers.join_next())
        .await?
        .ok_or("post-commit worker missing")???;
    assert!(
        receiver.try_recv().is_err(),
        "unexpected extra domain event"
    );
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end scenario follows durable evidence across fresh connections"
)]
async fn authenticated_command_flow(explicit: bool) -> TestResult<()> {
    let server = Server::authenticated_jetstream().await?;
    let connection = connect(&server.config(explicit)).await?;
    assert!(connection.client().server_info().auth_required);
    let context = ApplicationName::new(unique_name())?.bounded_context(Counter::BOUNDED_CONTEXT)?;
    let messaging =
        ApplicationMessagingConfig::new(context.application())?.with_max_bytes(8 * 1024 * 1024)?;
    let store_config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
    provision_application_messaging(connection.jetstream(), &messaging).await?;
    provision_event_store(connection.jetstream(), &store_config).await?;
    let store =
        NatsEventStore::connect(connection.jetstream().clone(), store_config.clone()).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let adapter = connection
        .messaging_adapter(messaging.topology().clone())
        .with_response_timeout(WAIT);
    let handler = Arc::new(adapter.command_handler(Arc::new(processor(&context, &store, &calls)?)));
    let bus = CommandBus::new(context.clone(), Arc::new(adapter));
    let operation = OperationId::new("authenticated-increment")?;
    let request = CommandRequest::new(
        operation.clone(),
        Increment {
            counter_id: "counter-1".to_owned(),
            amount: 7,
        },
    );
    let encoded = bus.encode(request.clone())?;
    let config = ConsumerConfig::new(
        context.consumer_name("increment", 1)?,
        context.durable_name("increment", 1)?,
        encoded.address().clone(),
        Duration::from_secs(10),
        Duration::from_secs(5),
        1,
        3,
    )?;
    provision_durable_consumer(connection.jetstream(), messaging.topology(), &config).await?;
    let consumer = connection
        .consumer_factory(messaging.topology().clone())
        .create(config.clone())?;
    let mut workers = JoinSet::new();
    workers.spawn(async move { consumer.run(handler).await });
    let result = tokio::select! {
        result = timeout(WAIT, bus.dispatch(request.clone())) => result??,
        result = workers.join_next() => return Err(format!("command worker stopped: {result:?}").into()),
    };
    assert_eq!(
        result.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    encoded.validate_response(result.response())?;
    assert!(!result.publication_duplicate());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    wait_for_ack(
        &connection,
        messaging.topology().command_stream().as_str(),
        config.durable_name().as_str(),
    )
    .await?;
    workers.shutdown().await;
    drop(bus);
    drop(store);
    connection.drain().await?;

    // No command worker is running. All following evidence comes from durable storage,
    // read through a new authenticated managed connection, not a local response cache.
    let replay_connection = connect(&server.config(explicit)).await?;
    assert!(replay_connection.client().server_info().auth_required);
    let replay_store =
        NatsEventStore::connect(replay_connection.jetstream().clone(), store_config.clone())
            .await?;
    let stream = StreamId::new(
        AggregateType::new(Counter::AGGREGATE_TYPE)?,
        AggregateId::new("counter-1")?,
    );
    let history = replay_store.load(&stream).await?;
    let [event] = history.as_slice() else {
        return Err("expected exactly one committed domain event".into());
    };
    assert_eq!(event.operation_id(), &operation);
    assert_eq!(event.correlation_id(), Some(encoded.correlation_id()));
    assert_eq!(
        event.causation_id().map(CausationId::as_str),
        Some(encoded.message_id().as_str())
    );
    let aggregate = CommandExecutor::new(replay_store.clone())
        .rehydrate::<Counter>(&stream)
        .await?;
    assert_eq!(*aggregate.state(), 7);
    let receipt = replay_store
        .load_transaction_receipt_in_context(context.name(), &operation)
        .await?
        .ok_or("missing persisted transaction receipt")?;
    assert_eq!(receipt.events(), history);
    assert_eq!(receipt.operation_fingerprint(), encoded.fingerprint());

    // Invoke a fresh processor so exact command replay is tested independently of
    // JetStream message deduplication and its already-persisted response.
    let replay = processor(&context, &replay_store, &calls)?
        .process(&encoded)
        .await?;
    assert_eq!(replay.outcome(), &CommandResponseOutcome::Accepted);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(replay_store.load(&stream).await?, history);
    let replay_bus = CommandBus::new(
        context.clone(),
        Arc::new(
            replay_connection
                .messaging_adapter(messaging.topology().clone())
                .with_response_timeout(WAIT),
        ),
    );
    let durable_response = timeout(WAIT, replay_bus.dispatch(request)).await??;
    assert_eq!(durable_response.response(), result.response());
    assert!(durable_response.publication_duplicate());

    // Start the post-commit consumer only now: the original authenticated writer
    // has disconnected, so observing its event requires durable historical delivery.
    consume_committed_history(&replay_connection, &context, &store_config, event).await?;
    replay_connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn authenticated_command_flow_with_explicit_credentials() -> TestResult<()> {
    timeout(Duration::from_secs(60), authenticated_command_flow(true)).await?
}

#[tokio::test]
async fn authenticated_command_flow_with_url_credentials() -> TestResult<()> {
    timeout(Duration::from_secs(60), authenticated_command_flow(false)).await?
}
