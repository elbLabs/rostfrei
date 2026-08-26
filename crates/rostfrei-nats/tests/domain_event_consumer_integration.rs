use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream::consumer;
use async_nats::{
    header::{
        NATS_BATCH_ID, NATS_BATCH_SEQUENCE, NATS_EXPECTED_LAST_SUBJECT_SEQUENCE,
        NATS_EXPECTED_STREAM, NATS_REQUIRED_API_LEVEL,
    },
    HeaderMap, Request,
};
use async_trait::async_trait;
use rostfrei::{Aggregate as RuntimeAggregate, Apply, Initialize};
use rostfrei_core::{
    AggregateId, AggregateType, CommittedDomainEvent, ContentFingerprint, DomainEventDispatcher,
    DomainEventHandler, DomainEventHandlerError, DomainEventHandlerErrorKind, EventBatch,
    EventCodec, EventStore, ExecutionMetadata, ExpectedVersion, JsonEventCodec, OperationId,
    StreamId,
};
use rostfrei_messaging_core::{ApplicationName, RetryDelay};
use rostfrei_nats::{
    provision_domain_event_consumer, provision_event_store, NatsDomainEventConsumer,
    NatsDomainEventConsumerConfig, NatsEventStore, NatsEventStoreConfig,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "domain-event-consumer", label = "Domain event consumer")]
struct TestContext;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = TestRoot)]
struct TestId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "consumer", label = "Consumer", owner = TestAggregate)]
struct TestRoot {
    #[rostfrei(identity)]
    id: TestId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "test-event", label = "Test event")]
struct TestEvent {
    value: String,
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "consumer",
    label = "Consumer",
    context = TestContext,
    root = TestRoot,
    events = [TestEvent]
)]
struct TestAggregate;

impl Initialize<TestAggregate> for TestRoot {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: TestId(stream_id.aggregate_id().as_str().to_owned()),
        }
    }
}

impl Apply<TestEvent> for TestRoot {
    fn apply(&mut self, _event: &TestEvent) {
        let _ = self.id.0.len();
    }
}

#[derive(Clone, Debug)]
struct HandledEvent {
    value: String,
    event_id: String,
    stream_id: StreamId,
    ordinal: u32,
}

struct RecordingHandler {
    sender: mpsc::UnboundedSender<HandledEvent>,
    failures_remaining: AtomicUsize,
    failure_kind: DomainEventHandlerErrorKind,
    calls: AtomicUsize,
}

impl RecordingHandler {
    fn new(sender: mpsc::UnboundedSender<HandledEvent>, failures: usize) -> Self {
        Self {
            sender,
            failures_remaining: AtomicUsize::new(failures),
            failure_kind: DomainEventHandlerErrorKind::Retryable,
            calls: AtomicUsize::new(0),
        }
    }

    fn blocking(sender: mpsc::UnboundedSender<HandledEvent>) -> Self {
        Self {
            sender,
            failures_remaining: AtomicUsize::new(1),
            failure_kind: DomainEventHandlerErrorKind::OperatorBlocking,
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl DomainEventHandler<TestEvent> for RecordingHandler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, TestEvent>,
    ) -> Result<(), DomainEventHandlerError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self
            .failures_remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(DomainEventHandlerError::new(
                self.failure_kind,
                "forced transient failure",
            ));
        }
        self.sender
            .send(HandledEvent {
                value: event.event().value.clone(),
                event_id: event.recorded().event_id().as_str().to_owned(),
                stream_id: event.recorded().stream_id().clone(),
                ordinal: event.recorded().commit_event_ordinal(),
            })
            .expect("recording receiver remains open");
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires NATS Server 2.12 configured by ROSTFREI_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn durable_domain_event_consumers_preserve_history_order_and_independent_progress() {
    let Ok(url) = std::env::var("ROSTFREI_NATS_URL") else {
        eprintln!("ROSTFREI_NATS_URL is not set; skipping real NATS integration test");
        return;
    };
    let client = async_nats::connect(url).await.expect("NATS connection");
    assert!(client.is_server_compatible(2, 12, 0));
    let context = async_nats::jetstream::new(client.clone());
    let suffix = unique_suffix();
    let bounded_context = ApplicationName::new(format!("rostfrei-{suffix}"))
        .expect("application name")
        .bounded_context("domain-event-consumer")
        .expect("bounded context");
    let event_store_config = NatsEventStoreConfig::new(
        &bounded_context,
        format!("DOMAIN_EVENT_CONSUMER_{suffix}").to_ascii_uppercase(),
    )
    .expect("event-store config")
    .with_storage_limits(64 * 1024 * 1024, 2 * 1024 * 1024)
    .expect("event-store storage limits");
    provision_event_store(&context, &event_store_config)
        .await
        .expect("event-store provisioning");
    let store = NatsEventStore::connect(context.clone(), event_store_config.clone())
        .await
        .expect("event store");

    let first_config = consumer_config(&bounded_context, &format!("history-{suffix}"));
    let second_config = consumer_config(&bounded_context, &format!("independent-{suffix}"));
    provision_domain_event_consumer(&context, &event_store_config, &first_config)
        .await
        .expect("first durable provisioning");
    provision_domain_event_consumer(&context, &event_store_config, &second_config)
        .await
        .expect("second durable provisioning");

    let (first_tx, mut first_rx) = mpsc::unbounded_channel();
    let first_handler = Arc::new(RecordingHandler::new(first_tx, 0));
    let first_consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        first_config.clone(),
        first_handler,
    )
    .await;
    let (first_shutdown_tx, first_shutdown_rx) = watch::channel(false);
    let mut first_task =
        tokio::spawn(async move { first_consumer.run_until_shutdown(first_shutdown_rx).await });

    let first_stream = stream("first");
    let first_outcome = store
        .append(
            &first_stream,
            ExpectedVersion::NoStream,
            batch(&first_stream, "first-operation", &["first"]),
        )
        .await
        .expect("committed event");
    let first_delivery = tokio::select! {
        delivery = receive(&mut first_rx) => delivery,
        result = &mut first_task => panic!("first durable stopped before delivery: {result:?}"),
    };
    assert_eq!(first_delivery.value, "first");
    assert_eq!(
        first_delivery.event_id,
        first_outcome.events()[0].event_id().as_str()
    );
    wait_for_ack(&context, &event_store_config, &first_config).await;

    let mut event_stream = context
        .get_stream(event_store_config.stream_name())
        .await
        .expect("event stream");
    let second_info = event_stream
        .get_consumer::<consumer::pull::Config>(second_config.durable_name().as_str())
        .await
        .expect("second durable")
        .info()
        .await
        .expect("second durable info")
        .clone();
    assert_eq!(second_info.num_ack_pending, 0);
    assert_eq!(
        second_info.num_pending, 1,
        "the first durable ACK must not advance the second"
    );

    let (second_tx, mut second_rx) = mpsc::unbounded_channel();
    let second_consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        second_config.clone(),
        Arc::new(RecordingHandler::new(second_tx, 0)),
    )
    .await;
    let (second_shutdown_tx, second_shutdown_rx) = watch::channel(false);
    let second_task =
        tokio::spawn(async move { second_consumer.run_until_shutdown(second_shutdown_rx).await });
    assert_eq!(receive(&mut second_rx).await.value, "first");
    wait_for_ack(&context, &event_store_config, &second_config).await;

    assert_eq!(
        store
            .load(&first_stream)
            .await
            .expect("authoritative history"),
        first_outcome.events(),
        "consumer ACKs must not remove aggregate history"
    );

    first_shutdown_tx.send(true).expect("first shutdown signal");
    second_shutdown_tx
        .send(true)
        .expect("second shutdown signal");
    first_task
        .await
        .expect("first task join")
        .expect("clean first shutdown");
    second_task
        .await
        .expect("second task join")
        .expect("clean second shutdown");

    let (restart_tx, mut restart_rx) = mpsc::unbounded_channel();
    let restarted = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        first_config.clone(),
        Arc::new(RecordingHandler::new(restart_tx, 0)),
    )
    .await;
    let (restart_shutdown_tx, restart_shutdown_rx) = watch::channel(false);
    let restart_task =
        tokio::spawn(async move { restarted.run_until_shutdown(restart_shutdown_rx).await });
    let next_stream = stream("next");
    store
        .append(
            &next_stream,
            ExpectedVersion::NoStream,
            batch(&next_stream, "next-operation", &["next"]),
        )
        .await
        .expect("next event");
    assert_eq!(receive(&mut restart_rx).await.value, "next");

    let ordered_stream = stream("ordered");
    store
        .append(
            &ordered_stream,
            ExpectedVersion::NoStream,
            batch(
                &ordered_stream,
                "ordered-operation",
                &["one", "two", "three"],
            ),
        )
        .await
        .expect("multi-event commit");
    let ordered = [
        receive(&mut restart_rx).await,
        receive(&mut restart_rx).await,
        receive(&mut restart_rx).await,
    ];
    let first_ordered_stream = ordered[0].stream_id.clone();
    assert_eq!(
        ordered.map(|event| (event.value, event.ordinal)),
        [
            ("one".to_owned(), 0),
            ("two".to_owned(), 1),
            ("three".to_owned(), 2),
        ]
    );
    assert!(ordered_stream != first_stream && first_ordered_stream == ordered_stream);
    restart_shutdown_tx
        .send(true)
        .expect("restart shutdown signal");
    restart_task
        .await
        .expect("restart task join")
        .expect("clean restart shutdown");

    let retry_config = consumer_config(&bounded_context, &format!("retry-{suffix}"));
    provision_domain_event_consumer(&context, &event_store_config, &retry_config)
        .await
        .expect("retry durable provisioning");
    let (retry_tx, mut retry_rx) = mpsc::unbounded_channel();
    let retry_handler = Arc::new(RecordingHandler::new(retry_tx, 1));
    let retry_consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        retry_config.clone(),
        retry_handler.clone(),
    )
    .await;
    let (retry_shutdown_tx, retry_shutdown_rx) = watch::channel(false);
    let retry_task =
        tokio::spawn(async move { retry_consumer.run_until_shutdown(retry_shutdown_rx).await });
    let retried = receive(&mut retry_rx).await;
    assert_eq!(
        retried.event_id,
        first_outcome.events()[0].event_id().as_str()
    );
    assert!(retry_handler.calls.load(Ordering::Relaxed) >= 2);
    retry_shutdown_tx.send(true).expect("retry shutdown signal");
    retry_task
        .await
        .expect("retry task join")
        .expect("clean retry shutdown");

    let blocked_config = NatsDomainEventConsumerConfig::new(
        bounded_context
            .consumer_name(&format!("blocked-{suffix}"), 1)
            .expect("blocked consumer name"),
        bounded_context
            .durable_name(&format!("blocked-{suffix}"), 1)
            .expect("blocked durable name"),
        Duration::from_secs(2),
        Duration::from_millis(500),
        RetryDelay::new(Duration::from_millis(50)).expect("retry delay"),
    )
    .expect("blocked consumer config");
    provision_domain_event_consumer(&context, &event_store_config, &blocked_config)
        .await
        .expect("blocked durable provisioning");
    let (blocked_tx, _blocked_rx) = mpsc::unbounded_channel();
    let blocked_consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        blocked_config.clone(),
        Arc::new(RecordingHandler::blocking(blocked_tx)),
    )
    .await;
    let (_blocked_shutdown_tx, blocked_shutdown_rx) = watch::channel(false);
    let blocked_result = tokio::time::timeout(
        Duration::from_secs(5),
        blocked_consumer.run_until_shutdown(blocked_shutdown_rx),
    )
    .await
    .expect("blocking handler timeout")
    .expect_err("blocking handler must stop its durable");
    assert_eq!(
        blocked_result.kind(),
        rostfrei_nats::DomainEventConsumerErrorKind::OperatorBlocked
    );

    let (unblocked_tx, mut unblocked_rx) = mpsc::unbounded_channel();
    let unblocked_consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        blocked_config,
        Arc::new(RecordingHandler::new(unblocked_tx, 0)),
    )
    .await;
    let (unblocked_shutdown_tx, unblocked_shutdown_rx) = watch::channel(false);
    let unblocked_task = tokio::spawn(async move {
        unblocked_consumer
            .run_until_shutdown(unblocked_shutdown_rx)
            .await
    });
    assert_eq!(
        receive(&mut unblocked_rx).await.event_id,
        first_outcome.events()[0].event_id().as_str(),
        "a restart must not skip and cumulatively ACK the blocked event"
    );
    unblocked_shutdown_tx
        .send(true)
        .expect("unblocked shutdown signal");
    unblocked_task
        .await
        .expect("unblocked task join")
        .expect("clean unblocked shutdown");

    stage_uncommitted_batch(&client, &event_store_config).await;
    let stream_info = event_stream.info().await.expect("stream info");
    assert_eq!(
        stream_info.state.messages, 5,
        "staged ADR-50 messages stay invisible"
    );
}

async fn connect_consumer(
    context: async_nats::jetstream::Context,
    event_store: NatsEventStoreConfig,
    config: NatsDomainEventConsumerConfig,
    handler: Arc<RecordingHandler>,
) -> NatsDomainEventConsumer {
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher
        .register::<TestAggregate, TestEvent, _>("test-event", handler)
        .expect("domain-event registration");
    NatsDomainEventConsumer::connect(context, event_store, config, Arc::new(dispatcher))
        .await
        .expect("domain-event consumer")
}

fn consumer_config(
    bounded_context: &rostfrei_messaging_core::BoundedContext,
    purpose: &str,
) -> NatsDomainEventConsumerConfig {
    NatsDomainEventConsumerConfig::new(
        bounded_context
            .consumer_name(purpose, 1)
            .expect("consumer name"),
        bounded_context
            .durable_name(purpose, 1)
            .expect("durable name"),
        Duration::from_secs(5),
        Duration::from_secs(2),
        RetryDelay::new(Duration::from_millis(50)).expect("retry delay"),
    )
    .expect("consumer config")
}

fn stream(id: &str) -> StreamId {
    StreamId::new(
        AggregateType::new(<TestAggregate as RuntimeAggregate>::aggregate_type().as_ref())
            .expect("aggregate type"),
        AggregateId::new(id).expect("aggregate ID"),
    )
}

fn batch(stream: &StreamId, operation: &str, values: &[&str]) -> EventBatch {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation).expect("operation ID"),
        ContentFingerprint::digest(operation),
    );
    let events = values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            let event: <TestAggregate as RuntimeAggregate>::Event = TestEvent {
                value: (*value).to_owned(),
            }
            .into();
            <JsonEventCodec as EventCodec<TestAggregate>>::encode(
                &JsonEventCodec,
                &event,
                metadata.event_id(u32::try_from(ordinal).expect("small test commit")),
            )
            .expect("encoded test event")
        })
        .collect();
    EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )
    .expect("event batch")
}

async fn receive(receiver: &mut mpsc::UnboundedReceiver<HandledEvent>) -> HandledEvent {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .expect("handler delivery timeout")
        .expect("handler delivery")
}

async fn wait_for_ack(
    context: &async_nats::jetstream::Context,
    event_store: &NatsEventStoreConfig,
    config: &NatsDomainEventConsumerConfig,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stream = context
                .get_stream(event_store.stream_name())
                .await
                .expect("event stream");
            let mut consumer = stream
                .get_consumer::<consumer::pull::Config>(config.durable_name().as_str())
                .await
                .expect("durable consumer");
            let info = consumer.info().await.expect("durable info");
            if info.num_ack_pending == 0 && info.num_pending == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("durable ACK timeout");
}

async fn stage_uncommitted_batch(client: &async_nats::Client, config: &NatsEventStoreConfig) {
    let staged_stream = stream("staged");
    let subject = config.aggregate_subject(
        staged_stream.aggregate_type().as_str(),
        staged_stream.aggregate_id().as_str(),
    );
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json");
    headers.insert(NATS_REQUIRED_API_LEVEL, "2");
    headers.insert(NATS_BATCH_ID, "staged-domain-event-test");
    headers.insert(NATS_BATCH_SEQUENCE, "1");
    headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
    headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
    let response = client
        .send_request(
            subject,
            Request::new()
                .headers(headers)
                .payload(br#"{"not":"committed"}"#.to_vec().into())
                .timeout(Some(Duration::from_secs(2))),
        )
        .await
        .expect("staged atomic request");
    assert!(response.payload.is_empty());
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}
