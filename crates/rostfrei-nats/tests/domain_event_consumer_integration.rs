#[path = "../src/hex.rs"]
mod hex;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_nats::jetstream::consumer;
use async_nats::{
    HeaderMap, Request,
    header::{
        NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL, NATS_BATCH_ID, NATS_BATCH_SEQUENCE,
        NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, NATS_EXPECTED_STREAM, NATS_REQUIRED_API_LEVEL,
    },
};
use async_trait::async_trait;
use base64::Engine as _;
use rostfrei::{Aggregate as RuntimeAggregate, Apply, Initialize};
use rostfrei_core::{
    AggregateId, AggregateType, CommittedDomainEvent, ContentFingerprint, DomainEventDispatcher,
    DomainEventHandler, DomainEventHandlerError, DomainEventHandlerErrorKind, EventBatch,
    EventCodec, EventStore, EventTransaction, ExecutionMetadata, ExpectedVersion, JsonEventCodec,
    OperationId, StreamId, TransactionParticipant,
};
use rostfrei_messaging_core::{ApplicationName, RetryDelay};
use rostfrei_nats::{
    NatsDomainEventConsumer, NatsDomainEventConsumerConfig, NatsEventStore, NatsEventStoreConfig,
    provision_domain_event_consumer, provision_event_store,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, watch};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "domain-event-consumer", label = "Domain event consumer")]
struct TestContext;

#[derive(rostfrei::DomainIdentity)]
struct TestId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "consumer", label = "Consumer")]
struct TestRoot {
    id: TestId,
}

impl rostfrei::EntityDefinition for TestRoot {
    type Owner = TestAggregate;
    type Identity = TestId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, rostfrei::DomainEvent)]
#[rostfrei(id = "test-event", label = "Test event")]
struct TestEvent {
    value: String,
}

#[derive(rostfrei::AggregateEvents)]
enum TestEvents {
    TestEvent(TestEvent),
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(id = "consumer", label = "Consumer")]
struct TestAggregate;

impl rostfrei::AggregateDefinition for TestAggregate {
    type Context = TestContext;
    type Root = TestRoot;
    type Event = TestEvents;
}

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
    failure_value: Option<String>,
    failure_kind: DomainEventHandlerErrorKind,
    calls: AtomicUsize,
}

impl RecordingHandler {
    const fn new(sender: mpsc::UnboundedSender<HandledEvent>, failures: usize) -> Self {
        Self {
            sender,
            failures_remaining: AtomicUsize::new(failures),
            failure_value: None,
            failure_kind: DomainEventHandlerErrorKind::Retryable,
            calls: AtomicUsize::new(0),
        }
    }

    fn failing_on_value(sender: mpsc::UnboundedSender<HandledEvent>, value: &str) -> Self {
        Self {
            sender,
            failures_remaining: AtomicUsize::new(1),
            failure_value: Some(value.to_owned()),
            failure_kind: DomainEventHandlerErrorKind::Retryable,
            calls: AtomicUsize::new(0),
        }
    }

    const fn blocking(sender: mpsc::UnboundedSender<HandledEvent>) -> Self {
        Self {
            sender,
            failures_remaining: AtomicUsize::new(1),
            failure_value: None,
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
        let failure_matches = self
            .failure_value
            .as_ref()
            .is_none_or(|value| value == &event.event().value);
        if failure_matches
            && self
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
            .map_err(|_| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    "recording receiver closed",
                )
            })?;
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires NATS Server 2.12.1 configured by ROSTFREI_NATS_URL"]
#[allow(clippy::too_many_lines)]
async fn durable_domain_event_consumers_preserve_history_order_and_independent_progress() {
    let Ok(url) = std::env::var("ROSTFREI_NATS_URL") else {
        eprintln!("ROSTFREI_NATS_URL is not set; skipping real NATS integration test");
        return;
    };
    let client = async_nats::connect(url).await.expect("NATS connection");
    assert!(client.is_server_compatible(2, 12, 1));
    let context = async_nats::jetstream::new(client.clone());
    let suffix = unique_suffix().expect("unique test suffix");
    let bounded_context = ApplicationName::new(format!("rostfrei-{suffix}"))
        .expect("application name")
        .bounded_context("domain-event-consumer")
        .expect("bounded context");
    let event_store_config = NatsEventStoreConfig::new(
        &bounded_context,
        format!("DOMAIN_EVENT_CONSUMER_{suffix}").to_ascii_uppercase(),
    )
    .expect("event-store config")
    .with_storage_limits(64 * 1024 * 1024, 512 * 1024)
    .expect("event-store storage limits");
    provision_event_store(&context, &event_store_config)
        .await
        .expect("event-store provisioning");
    let store = NatsEventStore::connect(context.clone(), event_store_config.clone())
        .await
        .expect("event store");

    let first_config = consumer_config(&bounded_context, &format!("history-{suffix}"))
        .expect("first consumer config");
    let second_config = consumer_config(&bounded_context, &format!("independent-{suffix}"))
        .expect("second consumer config");
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
    .await
    .expect("first consumer connection");
    let (first_shutdown_tx, first_shutdown_rx) = watch::channel(false);
    let mut first_task =
        tokio::spawn(async move { first_consumer.run_until_shutdown(first_shutdown_rx).await });

    let first_stream = stream("first").expect("first stream id");
    let first_outcome = store
        .append(
            &first_stream,
            ExpectedVersion::NoStream,
            batch(&first_stream, "first-operation", &["first"]).expect("first event batch"),
        )
        .await
        .expect("committed event");
    let first_delivery = tokio::select! {
        delivery = receive(&mut first_rx) => delivery.expect("first handler delivery"),
        result = &mut first_task => panic!("first durable stopped before delivery: {result:?}"),
    };
    assert_eq!(first_delivery.value, "first");
    assert_eq!(
        first_delivery.event_id,
        first_outcome.events()[0].event_id().as_str()
    );
    wait_for_ack(&context, &event_store_config, &first_config)
        .await
        .expect("first durable acknowledgement");

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
    .await
    .expect("second consumer connection");
    let (second_shutdown_tx, second_shutdown_rx) = watch::channel(false);
    let second_task =
        tokio::spawn(async move { second_consumer.run_until_shutdown(second_shutdown_rx).await });
    assert_eq!(
        receive(&mut second_rx)
            .await
            .expect("second handler delivery")
            .value,
        "first"
    );
    wait_for_ack(&context, &event_store_config, &second_config)
        .await
        .expect("second durable acknowledgement");

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
    .await
    .expect("restarted consumer connection");
    let (restart_shutdown_tx, restart_shutdown_rx) = watch::channel(false);
    let restart_task =
        tokio::spawn(async move { restarted.run_until_shutdown(restart_shutdown_rx).await });
    let next_stream = stream("next").expect("next stream id");
    store
        .append(
            &next_stream,
            ExpectedVersion::NoStream,
            batch(&next_stream, "next-operation", &["next"]).expect("next event batch"),
        )
        .await
        .expect("next event");
    assert_eq!(
        receive(&mut restart_rx)
            .await
            .expect("restarted consumer next delivery")
            .value,
        "next"
    );

    let ordered_stream = stream("ordered").expect("ordered stream id");
    store
        .append(
            &ordered_stream,
            ExpectedVersion::NoStream,
            batch(
                &ordered_stream,
                "ordered-operation",
                &["one", "two", "three"],
            )
            .expect("ordered event batch"),
        )
        .await
        .expect("multi-event commit");
    let ordered = [
        receive(&mut restart_rx)
            .await
            .expect("first ordered delivery"),
        receive(&mut restart_rx)
            .await
            .expect("second ordered delivery"),
        receive(&mut restart_rx)
            .await
            .expect("third ordered delivery"),
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

    let transaction_source = stream("transaction-source").expect("transaction source stream");
    let transaction_destination =
        stream("transaction-destination").expect("transaction destination stream");
    let transaction_operation = "cross-stream-operation";
    let transaction_outcome = store
        .append_transaction(EventTransaction::new(
            OperationId::new(transaction_operation).expect("transaction operation ID"),
            ContentFingerprint::digest(transaction_operation),
            vec![
                TransactionParticipant::new(
                    transaction_source.clone(),
                    ExpectedVersion::NoStream,
                    Some(
                        batch(
                            &transaction_source,
                            transaction_operation,
                            &["source-event"],
                        )
                        .expect("source transaction event batch"),
                    ),
                ),
                TransactionParticipant::new(
                    transaction_destination.clone(),
                    ExpectedVersion::NoStream,
                    Some(
                        batch(
                            &transaction_destination,
                            transaction_operation,
                            &["destination-event"],
                        )
                        .expect("destination transaction event batch"),
                    ),
                ),
            ],
        ))
        .await
        .expect("cross-stream transaction");
    assert_eq!(transaction_outcome.receipt().events().len(), 2);
    let transaction_final_sequence = event_stream
        .get_last_raw_message_by_subject(&event_store_config.aggregate_subject(
            transaction_destination.aggregate_type().as_str(),
            transaction_destination.aggregate_id().as_str(),
        ))
        .await
        .expect("final transaction event")
        .sequence;
    let transaction_deliveries = [
        receive(&mut restart_rx)
            .await
            .expect("source transaction delivery"),
        receive(&mut restart_rx)
            .await
            .expect("destination transaction delivery"),
    ];
    assert_eq!(
        transaction_deliveries
            .iter()
            .map(|event| (event.value.as_str(), &event.stream_id, event.ordinal))
            .collect::<Vec<_>>(),
        vec![
            ("source-event", &transaction_source, 0),
            ("destination-event", &transaction_destination, 0),
        ]
    );
    wait_for_ack_floor(
        &context,
        &event_store_config,
        &first_config,
        transaction_final_sequence,
    )
    .await
    .expect("transaction durable acknowledgement");
    restart_shutdown_tx
        .send(true)
        .expect("restart shutdown signal");
    restart_task
        .await
        .expect("restart task join")
        .expect("clean restart shutdown");

    let (transaction_restart_tx, mut transaction_restart_rx) = mpsc::unbounded_channel();
    let transaction_restart = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        first_config.clone(),
        Arc::new(RecordingHandler::new(transaction_restart_tx, 0)),
    )
    .await
    .expect("transaction restart consumer connection");
    let (transaction_restart_shutdown_tx, transaction_restart_shutdown_rx) = watch::channel(false);
    let mut transaction_restart_task = tokio::spawn(async move {
        transaction_restart
            .run_until_shutdown(transaction_restart_shutdown_rx)
            .await
    });
    let after_transaction_stream = stream("after-transaction").expect("post-transaction stream");
    store
        .append(
            &after_transaction_stream,
            ExpectedVersion::NoStream,
            batch(
                &after_transaction_stream,
                "after-transaction-operation",
                &["after-transaction"],
            )
            .expect("post-transaction event batch"),
        )
        .await
        .expect("post-transaction event");
    let after_transaction = tokio::select! {
        delivery = receive(&mut transaction_restart_rx) => {
            delivery.expect("post-transaction delivery")
        },
        result = &mut transaction_restart_task => {
            panic!("transaction durable stopped after restart: {result:?}")
        }
    };
    assert_eq!(
        after_transaction.value, "after-transaction",
        "the transaction must not be redelivered after its durable ACK floor is persisted"
    );
    wait_for_ack(&context, &event_store_config, &first_config)
        .await
        .expect("post-transaction durable acknowledgement");
    transaction_restart_shutdown_tx
        .send(true)
        .expect("transaction restart shutdown signal");
    transaction_restart_task
        .await
        .expect("transaction restart task join")
        .expect("clean transaction restart shutdown");

    let retry_config = consumer_config(&bounded_context, &format!("retry-{suffix}"))
        .expect("retry consumer config");
    provision_domain_event_consumer(&context, &event_store_config, &retry_config)
        .await
        .expect("retry durable provisioning");
    let (retry_tx, mut retry_rx) = mpsc::unbounded_channel();
    let retry_handler = Arc::new(RecordingHandler::failing_on_value(
        retry_tx,
        "destination-event",
    ));
    let retry_consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        retry_config.clone(),
        retry_handler.clone(),
    )
    .await
    .expect("retry consumer connection");
    let (retry_shutdown_tx, retry_shutdown_rx) = watch::channel(false);
    let retry_task =
        tokio::spawn(async move { retry_consumer.run_until_shutdown(retry_shutdown_rx).await });
    let retry_deliveries = [
        receive(&mut retry_rx).await.expect("first retry delivery"),
        receive(&mut retry_rx).await.expect("next retry delivery"),
        receive(&mut retry_rx)
            .await
            .expect("ordered retry delivery 1"),
        receive(&mut retry_rx)
            .await
            .expect("ordered retry delivery 2"),
        receive(&mut retry_rx)
            .await
            .expect("ordered retry delivery 3"),
        receive(&mut retry_rx)
            .await
            .expect("first source transaction delivery"),
        receive(&mut retry_rx)
            .await
            .expect("retried source transaction delivery"),
        receive(&mut retry_rx)
            .await
            .expect("destination transaction delivery"),
        receive(&mut retry_rx)
            .await
            .expect("post-transaction retry delivery"),
    ];
    assert_eq!(
        retry_deliveries
            .iter()
            .map(|event| event.value.as_str())
            .collect::<Vec<_>>(),
        vec![
            "first",
            "next",
            "one",
            "two",
            "three",
            "source-event",
            "source-event",
            "destination-event",
            "after-transaction",
        ],
        "failure on the later transaction event must retry the whole transaction"
    );
    let retried_sources = retry_deliveries
        .iter()
        .filter(|event| event.value == "source-event")
        .collect::<Vec<_>>();
    assert_eq!(retried_sources.len(), 2);
    assert!(
        retried_sources
            .windows(2)
            .all(|events| events[0].event_id == events[1].event_id)
    );
    assert!(retry_handler.calls.load(Ordering::Relaxed) >= 10);
    wait_for_ack(&context, &event_store_config, &retry_config)
        .await
        .expect("retry durable acknowledgement");
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
    .await
    .expect("blocked consumer connection");
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
    .await
    .expect("unblocked consumer connection");
    let (unblocked_shutdown_tx, unblocked_shutdown_rx) = watch::channel(false);
    let unblocked_task = tokio::spawn(async move {
        unblocked_consumer
            .run_until_shutdown(unblocked_shutdown_rx)
            .await
    });
    assert_eq!(
        receive(&mut unblocked_rx)
            .await
            .expect("unblocked handler delivery")
            .event_id,
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

    let staged_response_payload = stage_uncommitted_batch(&client, &event_store_config)
        .await
        .expect("stage uncommitted batch");
    assert!(staged_response_payload.is_empty());
    let stream_info = event_stream.info().await.expect("stream info");
    assert_eq!(
        stream_info.state.messages, 9,
        "staged ADR-50 messages stay invisible"
    );
}

#[tokio::test]
#[ignore = "requires NATS Server 2.12.1 configured by ROSTFREI_NATS_URL"]
async fn schema_four_events_without_a_valid_receipt_are_not_dispatched() -> TestResult<()> {
    let Ok(url) = std::env::var("ROSTFREI_NATS_URL") else {
        eprintln!("ROSTFREI_NATS_URL is not set; skipping real NATS integration test");
        return Ok(());
    };
    let client = async_nats::connect(url).await?;
    if !client.is_server_compatible(2, 12, 1) {
        return Err("NATS Server 2.12.1 or newer is required".into());
    }
    let context = async_nats::jetstream::new(client.clone());
    let suffix = unique_suffix()?;
    let bounded_context = ApplicationName::new(format!("rostfrei-receipt-{suffix}"))?
        .bounded_context("domain-event-consumer")?;
    let event_store_config = NatsEventStoreConfig::new(
        &bounded_context,
        format!("DOMAIN_EVENT_RECEIPT_{suffix}").to_ascii_uppercase(),
    )?
    .with_storage_limits(64 * 1024 * 1024, 512 * 1024)?;
    provision_event_store(&context, &event_store_config).await?;
    let config = consumer_config(&bounded_context, &format!("receipt-{suffix}"))?;
    provision_domain_event_consumer(&context, &event_store_config, &config).await?;

    let (sender, mut receiver) = mpsc::unbounded_channel();
    let handler = Arc::new(RecordingHandler::new(sender, 0));
    let consumer = connect_consumer(
        context.clone(),
        event_store_config.clone(),
        config,
        handler.clone(),
    )
    .await?;
    let forged_stream = stream("missing-receipt")?;
    commit_schema_four_event_without_receipt(&client, &event_store_config, &forged_stream).await?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        consumer.run_until_shutdown(shutdown_rx),
    )
    .await?;
    let error = result
        .err()
        .ok_or("a schema-4 event without a receipt did not stop the durable")?;
    if error.kind() != rostfrei_nats::DomainEventConsumerErrorKind::InvalidCommittedEvent {
        return Err(format!("consumer returned the wrong error: {error}").into());
    }
    if handler.calls.load(Ordering::Relaxed) != 0 {
        return Err("the handler was called for a receiptless schema-4 event".into());
    }
    if !matches!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Empty)) {
        return Err("a receiptless schema-4 event was dispatched".into());
    }
    drop(shutdown_tx);
    Ok(())
}

async fn connect_consumer(
    context: async_nats::jetstream::Context,
    event_store: NatsEventStoreConfig,
    config: NatsDomainEventConsumerConfig,
    handler: Arc<RecordingHandler>,
) -> TestResult<NatsDomainEventConsumer> {
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register::<TestAggregate, TestEvent, _>("test-event", handler)?;
    Ok(
        NatsDomainEventConsumer::connect(context, event_store, config, Arc::new(dispatcher))
            .await?,
    )
}

fn consumer_config(
    bounded_context: &rostfrei_messaging_core::BoundedContext,
    purpose: &str,
) -> TestResult<NatsDomainEventConsumerConfig> {
    Ok(NatsDomainEventConsumerConfig::new(
        bounded_context.consumer_name(purpose, 1)?,
        bounded_context.durable_name(purpose, 1)?,
        Duration::from_secs(5),
        Duration::from_secs(2),
        RetryDelay::new(Duration::from_millis(50))?,
    )?)
}

fn stream(id: &str) -> TestResult<StreamId> {
    Ok(StreamId::new(
        AggregateType::new(<TestAggregate as RuntimeAggregate>::aggregate_type().as_ref())?,
        AggregateId::new(id)?,
    ))
}

fn batch(stream: &StreamId, operation: &str, values: &[&str]) -> TestResult<EventBatch> {
    let metadata = ExecutionMetadata::new(
        stream.clone(),
        OperationId::new(operation)?,
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
            let ordinal = u32::try_from(ordinal)?;
            Ok(<JsonEventCodec as EventCodec<TestAggregate>>::encode(
                &JsonEventCodec,
                &event,
                metadata.event_id(ordinal),
            )?)
        })
        .collect::<TestResult<Vec<_>>>()?;
    Ok(EventBatch::new(
        metadata.commit_id().clone(),
        metadata.operation_id().clone(),
        metadata.operation_fingerprint(),
        events,
    )?)
}

async fn receive(receiver: &mut mpsc::UnboundedReceiver<HandledEvent>) -> TestResult<HandledEvent> {
    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await?
        .ok_or_else(|| std::io::Error::other("handler delivery channel closed").into())
}

async fn wait_for_ack(
    context: &async_nats::jetstream::Context,
    event_store: &NatsEventStoreConfig,
    config: &NatsDomainEventConsumerConfig,
) -> TestResult<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stream = context.get_stream(event_store.stream_name()).await?;
            let mut consumer = stream
                .get_consumer::<consumer::pull::Config>(config.durable_name().as_str())
                .await?;
            let info = consumer.info().await?;
            if info.num_ack_pending == 0 && info.num_pending == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        TestResult::Ok(())
    })
    .await??;
    Ok(())
}

async fn wait_for_ack_floor(
    context: &async_nats::jetstream::Context,
    event_store: &NatsEventStoreConfig,
    config: &NatsDomainEventConsumerConfig,
    expected_stream_sequence: u64,
) -> TestResult<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stream = context.get_stream(event_store.stream_name()).await?;
            let mut consumer = stream
                .get_consumer::<consumer::pull::Config>(config.durable_name().as_str())
                .await?;
            let info = consumer.info().await?;
            if info.ack_floor.stream_sequence >= expected_stream_sequence
                && info.num_ack_pending == 0
                && info.num_pending == 0
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        TestResult::Ok(())
    })
    .await??;
    Ok(())
}

async fn stage_uncommitted_batch(
    client: &async_nats::Client,
    config: &NatsEventStoreConfig,
) -> TestResult<Vec<u8>> {
    let staged_stream = stream("staged")?;
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
        .await?;
    Ok(response.payload.to_vec())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedStoredEventWire<'a> {
    schema_version: u16,
    checksum: String,
    event: &'a ForgedStoredEventContent<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedStoredEventContent<'a> {
    event_store_stream: &'a str,
    application: &'a str,
    bounded_context: &'a str,
    stream: ForgedStreamIdentity<'a>,
    stream_version: u64,
    commit_id: &'a str,
    operation_id: &'a str,
    operation_fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    causation_id: Option<&'a str>,
    commit_event_ordinal: u32,
    commit_event_count: u32,
    transaction_event_ordinal: u32,
    transaction_event_count: u32,
    event_id: &'a str,
    event_type: &'a str,
    event_schema_version: u32,
    payload_base64: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedStreamIdentity<'a> {
    aggregate_type: &'a str,
    aggregate_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ForgedEventChecksumInput<'a> {
    schema_version: u16,
    event: &'a ForgedStoredEventContent<'a>,
}

async fn commit_schema_four_event_without_receipt(
    client: &async_nats::Client,
    config: &NatsEventStoreConfig,
    stream_id: &StreamId,
) -> TestResult<()> {
    const SCHEMA_VERSION: u16 = 4;
    let event_batch = batch(
        stream_id,
        "schema-four-without-receipt",
        &["must-not-dispatch"],
    )?;
    let event = event_batch
        .events()
        .first()
        .ok_or("forged event batch is empty")?;
    let content = ForgedStoredEventContent {
        event_store_stream: config.stream_name(),
        application: config.application().as_str(),
        bounded_context: config.bounded_context().as_str(),
        stream: ForgedStreamIdentity {
            aggregate_type: stream_id.aggregate_type().as_str(),
            aggregate_id: stream_id.aggregate_id().as_str(),
        },
        stream_version: 1,
        commit_id: event_batch.commit_id().as_str(),
        operation_id: event_batch.operation_id().as_str(),
        operation_fingerprint: event_batch.operation_fingerprint().to_hex(),
        correlation_id: None,
        causation_id: None,
        commit_event_ordinal: 0,
        commit_event_count: 1,
        transaction_event_ordinal: 0,
        transaction_event_count: 1,
        event_id: event.event_id().as_str(),
        event_type: event.event_type(),
        event_schema_version: event.schema_version(),
        payload_base64: base64::engine::general_purpose::STANDARD.encode(event.payload()),
    };
    let checksum_input = serde_json::to_vec(&ForgedEventChecksumInput {
        schema_version: SCHEMA_VERSION,
        event: &content,
    })?;
    let payload = serde_json::to_vec(&ForgedStoredEventWire {
        schema_version: SCHEMA_VERSION,
        checksum: hex::encode_lower_hex(Sha256::digest(checksum_input)),
        event: &content,
    })?;
    let batch_id = "schema-four-with-unrelated-filler";
    let subject = config.aggregate_subject(
        stream_id.aggregate_type().as_str(),
        stream_id.aggregate_id().as_str(),
    );
    let mut event_headers = HeaderMap::new();
    event_headers.insert("Content-Type", "application/json");
    event_headers.insert(NATS_REQUIRED_API_LEVEL, "2");
    event_headers.insert(NATS_BATCH_ID, batch_id);
    event_headers.insert(NATS_BATCH_SEQUENCE, "1");
    event_headers.insert(NATS_EXPECTED_STREAM, config.stream_name());
    event_headers.insert(NATS_EXPECTED_LAST_SUBJECT_SEQUENCE, "0");
    let staged = client
        .send_request(
            subject,
            Request::new()
                .headers(event_headers)
                .payload(payload.into())
                .timeout(Some(config.puback_timeout())),
        )
        .await?;
    if !staged.payload.is_empty() {
        return Err("NATS acknowledged the forged event before its filler".into());
    }

    let mut filler_headers = HeaderMap::new();
    filler_headers.insert("Content-Type", "application/json");
    filler_headers.insert(NATS_REQUIRED_API_LEVEL, "2");
    filler_headers.insert(NATS_BATCH_ID, batch_id);
    filler_headers.insert(NATS_BATCH_SEQUENCE, "2");
    filler_headers.insert(NATS_BATCH_COMMIT, NATS_BATCH_COMMIT_FINAL);
    let committed = client
        .send_request(
            config.transaction_guard_subject(stream_id, "unrelated-filler", 0),
            Request::new()
                .headers(filler_headers)
                .payload(br#"{"unrelated":true}"#.to_vec().into())
                .timeout(Some(config.puback_timeout())),
        )
        .await?;
    if committed.payload.is_empty() {
        return Err("NATS did not commit the forged event with its filler".into());
    }
    Ok(())
}

fn unique_suffix() -> TestResult<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(format!("{}-{nanos}", std::process::id()))
}
rostfrei::install_macro_support!();
