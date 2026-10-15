#![allow(
    clippy::panic_in_result_fn,
    reason = "assertions report broker contract violations"
)]

use futures_util::TryStreamExt as _;
use tokio::{
    sync::{Notify, Semaphore},
    time::timeout,
};

use super::*;

impl rostfrei::IntegrationEvent for TestEvent {
    const EVENT_NAME: &'static str = "published-test-event";
    const SCHEMA_VERSION: u32 = 1;
}

struct Fixture {
    url: String,
    context: async_nats::jetstream::Context,
    event_store: NatsEventStoreConfig,
    config: NatsDomainEventConsumerConfig,
    store: NatsEventStore,
}

impl Fixture {
    async fn new(ack_wait: Duration, processing_timeout: Duration) -> TestResult<Self> {
        let url = rostfrei_testing::integration::nats_url()?;
        let context = async_nats::jetstream::new(async_nats::connect(&url).await?);
        let suffix = unique_suffix()?;
        let scope = ApplicationName::new(format!("pool-{suffix}"))?
            .bounded_context("domain-event-consumer")?;
        let event_store =
            NatsEventStoreConfig::new(&scope, format!("POOL_{suffix}").to_ascii_uppercase())?
                .with_storage_limits(64 * 1024 * 1024, 512 * 1024)?;
        provision_event_store(&context, &event_store).await?;
        let store = NatsEventStore::connect(context.clone(), event_store.clone()).await?;
        let config = NatsDomainEventConsumerConfig::new(
            scope.consumer_name("publication", 1)?,
            scope.durable_name("publication", 1)?,
            ack_wait,
            processing_timeout,
            RetryDelay::new(Duration::from_millis(20))?,
        )?;
        provision_domain_event_consumer(&context, &event_store, &config).await?;
        Ok(Self {
            url,
            context,
            event_store,
            config,
            store,
        })
    }

    async fn worker<H: DomainEventHandler<TestEvent> + 'static>(
        &self,
        handler: Arc<H>,
    ) -> TestResult<NatsDomainEventConsumer> {
        connect_consumer(
            async_nats::jetstream::new(async_nats::connect(&self.url).await?),
            self.event_store.clone(),
            self.config.clone(),
            handler,
        )
        .await
    }

    async fn append(&self, operation: &str, values: &[&str]) -> TestResult<()> {
        let id = stream(operation)?;
        self.store
            .append(
                &id,
                ExpectedVersion::NoStream,
                batch(&id, operation, values)?,
            )
            .await?;
        Ok(())
    }

    async fn info(&self) -> TestResult<consumer::Info> {
        let stream = self
            .context
            .get_stream(self.event_store.stream_name())
            .await?;
        let durable: consumer::PullConsumer = stream
            .get_consumer(self.config.durable_name().as_str())
            .await?;
        Ok(durable.cached_info().clone())
    }

    async fn wait_for_waiters(&self, count: usize) -> TestResult<()> {
        timeout(Duration::from_secs(3), async {
            loop {
                if self.info().await?.num_waiting >= count {
                    return TestResult::Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?
    }

    async fn drain(&self) -> TestResult<()> {
        wait_for_ack(&self.context, &self.event_store, &self.config).await
    }

    async fn cleanup(&self) -> TestResult<()> {
        self.context
            .delete_stream(self.event_store.stream_name())
            .await?;
        Ok(())
    }
}

struct GatedHandler {
    recording: RecordingHandler,
    value: &'static str,
    entered: Notify,
    release: Semaphore,
    fail: bool,
}

impl GatedHandler {
    fn new(sender: mpsc::UnboundedSender<HandledEvent>, value: &'static str, fail: bool) -> Self {
        Self {
            recording: RecordingHandler::new(sender, 0),
            value,
            entered: Notify::new(),
            release: Semaphore::new(0),
            fail,
        }
    }
}

#[async_trait]
impl DomainEventHandler<TestEvent> for GatedHandler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, TestEvent>,
    ) -> Result<(), DomainEventHandlerError> {
        self.recording.handle(event).await?;
        if event.event().value == self.value {
            self.entered.notify_one();
            self.release
                .acquire()
                .await
                .map_err(|error| {
                    DomainEventHandlerError::new(
                        DomainEventHandlerErrorKind::OperatorBlocking,
                        error.to_string(),
                    )
                })?
                .forget();
            if self.fail {
                return Err(DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::Retryable,
                    "interrupted work",
                ));
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn overlapping_workers_share_a_durable_and_preserve_multi_aggregate_order() -> TestResult<()>
{
    let fixture = Fixture::new(Duration::from_secs(10), Duration::from_secs(5)).await?;
    let (sender, mut received) = mpsc::unbounded_channel();
    let first = fixture
        .worker(Arc::new(RecordingHandler::new(sender.clone(), 0)))
        .await?;
    let second = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await?;
    let (stop, stopping) = watch::channel(false);
    let a = tokio::spawn(first.run_until_shutdown(stopping.clone()));
    let b = tokio::spawn(second.run_until_shutdown(stopping));
    // Both pulls must coexist on an empty durable (the original rollout failure).
    fixture.wait_for_waiters(2).await?;
    for operation in ["a", "b"] {
        let source = stream(&format!("{operation}-source"))?;
        let target = stream(&format!("{operation}-target"))?;
        fixture
            .store
            .append_transaction(EventTransaction::new(
                OperationId::new(operation)?,
                ContentFingerprint::digest(operation),
                vec![
                    TransactionParticipant::new(
                        source.clone(),
                        ExpectedVersion::NoStream,
                        Some(batch(&source, operation, &["one", "two"])?),
                    ),
                    TransactionParticipant::new(
                        target.clone(),
                        ExpectedVersion::NoStream,
                        Some(batch(&target, operation, &["three"])?),
                    ),
                ],
            ))
            .await?;
        for expected in ["one", "two", "three"] {
            let event = receive(&mut received).await?;
            assert_eq!(event.value, expected);
            assert_eq!(
                event.stream_id,
                if expected == "three" {
                    target.clone()
                } else {
                    source.clone()
                }
            );
        }
    }
    let large: Vec<String> = (0..rostfrei_core::MAX_EVENTS_PER_BATCH)
        .map(|ordinal| format!("large-{ordinal}"))
        .collect();
    fixture
        .append(
            "large",
            &large.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .await?;
    for expected in large {
        assert_eq!(receive(&mut received).await?.value, expected);
    }
    fixture.drain().await?;
    assert!(
        received.try_recv().is_err(),
        "tail deliveries must not republish a completed transaction"
    );
    let info = fixture.info().await?;
    assert_eq!(info.config.ack_policy, consumer::AckPolicy::Explicit);
    assert_eq!(info.config.max_ack_pending, 1);
    stop.send(true)?;
    timeout(Duration::from_secs(3), a).await???;
    timeout(Duration::from_secs(3), b).await???;
    fixture.cleanup().await
}

#[tokio::test]
async fn graceful_shutdown_finishes_the_transaction_and_restart_only_acks_its_tail()
-> TestResult<()> {
    let fixture = Fixture::new(Duration::from_secs(10), Duration::from_secs(5)).await?;
    fixture
        .append("transaction", &["one", "two", "three"])
        .await?;
    fixture.append("next", &["next"]).await?;
    let (sender, mut received) = mpsc::unbounded_channel();
    let handler = Arc::new(GatedHandler::new(sender, "two", false));
    let worker = fixture.worker(handler.clone()).await?;
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    timeout(Duration::from_secs(3), handler.entered.notified()).await?;
    let info = fixture.info().await?;
    assert_eq!(info.num_ack_pending, 1);
    assert_eq!(
        info.ack_floor.stream_sequence, 0,
        "no ACK before the entire transaction succeeds"
    );
    stop.send(true)?;
    handler.release.add_permits(1);
    timeout(Duration::from_secs(3), task).await???;
    for expected in ["one", "two", "three"] {
        assert_eq!(receive(&mut received).await?.value, expected);
    }
    let info = fixture.info().await?;
    assert_eq!(info.ack_floor.stream_sequence, 1);
    assert_eq!(info.num_pending, 3, "shutdown must not start another pull");

    let (sender, mut received) = mpsc::unbounded_channel();
    let worker = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await?;
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    assert_eq!(receive(&mut received).await?.value, "next");
    fixture.drain().await?;
    assert!(received.try_recv().is_err());
    stop.send(true)?;
    timeout(Duration::from_secs(3), task).await???;
    fixture.cleanup().await
}

#[tokio::test]
async fn crash_redelivers_the_whole_transaction_before_later_work() -> TestResult<()> {
    let fixture = Fixture::new(Duration::from_millis(800), Duration::from_millis(500)).await?;
    fixture
        .append("transaction", &["one", "two", "three"])
        .await?;
    fixture.append("next", &["next"]).await?;
    let (sender, mut received) = mpsc::unbounded_channel();
    let handler = Arc::new(GatedHandler::new(sender, "two", false));
    let worker = fixture.worker(handler.clone()).await?;
    let (_stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    timeout(Duration::from_secs(3), handler.entered.notified()).await?;
    let original = receive(&mut received).await?;
    assert_eq!(original.value, "one");
    assert_eq!(receive(&mut received).await?.value, "two");
    task.abort();
    assert!(task.await.is_err());

    let (sender, mut received) = mpsc::unbounded_channel();
    let worker = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await?;
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    let replay = receive(&mut received).await?;
    assert_eq!(replay.event_id, original.event_id);
    for expected in ["two", "three", "next"] {
        assert_eq!(receive(&mut received).await?.value, expected);
    }
    fixture.drain().await?;
    assert!(received.try_recv().is_err());
    stop.send(true)?;
    timeout(Duration::from_secs(3), task).await???;
    fixture.cleanup().await
}

#[tokio::test]
async fn failed_work_on_shutdown_is_released_to_a_waiting_worker_without_ack_wait() -> TestResult<()>
{
    let fixture = Fixture::new(Duration::from_secs(30), Duration::from_secs(5)).await?;
    fixture.append("transaction", &["one", "two"]).await?;
    let (sender, mut received) = mpsc::unbounded_channel();
    let handler = Arc::new(GatedHandler::new(sender, "two", true));
    let worker = fixture.worker(handler.clone()).await?;
    let (stop, stopping) = watch::channel(false);
    let old = tokio::spawn(worker.run_until_shutdown(stopping));
    timeout(Duration::from_secs(3), handler.entered.notified()).await?;
    assert_eq!(receive(&mut received).await?.value, "one");
    assert_eq!(receive(&mut received).await?.value, "two");
    let (sender, mut replay) = mpsc::unbounded_channel();
    let worker = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await?;
    let (stop_new, stopping) = watch::channel(false);
    let new = tokio::spawn(worker.run_until_shutdown(stopping));
    fixture.wait_for_waiters(1).await?;
    assert!(replay.try_recv().is_err());
    stop.send(true)?;
    handler.release.add_permits(1);
    timeout(Duration::from_secs(3), old).await???;
    assert_eq!(receive(&mut replay).await?.value, "one");
    assert_eq!(receive(&mut replay).await?.value, "two");
    fixture.drain().await?;
    stop_new.send(true)?;
    timeout(Duration::from_secs(3), new).await???;
    fixture.cleanup().await
}

#[tokio::test]
async fn idle_shutdown_returns_a_late_delivery_without_dispatching_it() -> TestResult<()> {
    let fixture = Fixture::new(Duration::from_secs(30), Duration::from_secs(5)).await?;
    let (sender, mut received) = mpsc::unbounded_channel();
    let worker = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await?;
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    fixture.wait_for_waiters(1).await?;
    stop.send(true)?;
    fixture.append("late", &["late"]).await?;
    timeout(Duration::from_secs(3), task).await???;
    assert!(received.try_recv().is_err());

    let stream = fixture
        .context
        .get_stream(fixture.event_store.stream_name())
        .await?;
    let consumer: consumer::PullConsumer = stream
        .get_consumer(fixture.config.durable_name().as_str())
        .await?;
    let message = consumer
        .batch()
        .max_messages(1)
        .expires(Duration::from_secs(2))
        .messages()
        .await?
        .try_next()
        .await?
        .ok_or("late delivery was stranded until AckWait")?;
    message.double_ack().await?;
    fixture.cleanup().await
}

#[tokio::test]
async fn explicit_offline_migration_preserves_confirmed_progress() -> TestResult<()> {
    let mut fixture = Fixture::new(Duration::from_secs(10), Duration::from_secs(5)).await?;
    fixture.append("processed", &["processed"]).await?;
    fixture.append("pending", &["one", "two"]).await?;
    let stream = fixture
        .context
        .get_stream(fixture.event_store.stream_name())
        .await?;
    stream
        .delete_consumer(fixture.config.durable_name().as_str())
        .await?;
    let old: consumer::PullConsumer = stream
        .create_consumer(consumer::pull::Config {
            durable_name: Some(fixture.config.durable_name().as_str().to_owned()),
            ack_policy: consumer::AckPolicy::All,
            max_waiting: 1,
            filter_subject: fixture.event_store.aggregate_subject_filter(),
            ..Default::default()
        })
        .await?;
    let message = old
        .batch()
        .max_messages(1)
        .expires(Duration::from_secs(1))
        .messages()
        .await?
        .try_next()
        .await?
        .ok_or("missing legacy delivery")?;
    message.double_ack().await?;
    let (sender, _received) = mpsc::unbounded_channel();
    let error = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await
        .err()
        .ok_or("legacy durable was silently accepted")?;
    assert!(error.to_string().contains("offline migration"));
    let saved = old.get_info().await?.ack_floor.stream_sequence;
    assert_eq!(saved, 1);
    stream
        .delete_consumer(fixture.config.durable_name().as_str())
        .await?;
    fixture.config = fixture
        .config
        .with_start_sequence(saved.checked_add(1).ok_or("sequence overflow")?)?;
    provision_domain_event_consumer(&fixture.context, &fixture.event_store, &fixture.config)
        .await?;
    let (sender, mut received) = mpsc::unbounded_channel();
    let worker = fixture
        .worker(Arc::new(RecordingHandler::new(sender, 0)))
        .await?;
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    for expected in ["one", "two"] {
        assert_eq!(receive(&mut received).await?.value, expected);
    }
    fixture.drain().await?;
    assert!(received.try_recv().is_err());
    stop.send(true)?;
    timeout(Duration::from_secs(3), task).await???;
    fixture.cleanup().await
}

struct PublishingHandler {
    bus: rostfrei::IntegrationEventBus,
    receipts: mpsc::UnboundedSender<(String, bool)>,
    pause: bool,
    published: Notify,
}

#[async_trait]
impl DomainEventHandler<TestEvent> for PublishingHandler {
    async fn handle(
        &self,
        event: &CommittedDomainEvent<'_, TestEvent>,
    ) -> Result<(), DomainEventHandlerError> {
        let committed =
            rostfrei::CommittedEventContext::new(event.recorded()).map_err(|error| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::InvalidCommittedEvent,
                    error.to_string(),
                )
            })?;
        let receipt = self
            .bus
            .publish(committed, event.event().clone())
            .await
            .map_err(|error| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::Retryable,
                    error.to_string(),
                )
            })?;
        self.receipts
            .send((
                receipt.message_id().as_str().to_owned(),
                receipt.duplicate(),
            ))
            .map_err(|error| {
                DomainEventHandlerError::new(
                    DomainEventHandlerErrorKind::OperatorBlocking,
                    error.to_string(),
                )
            })?;
        if self.pause {
            self.published.notify_one();
            // Simulate a worker disappearing after PubAck but before source ACK.
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

async fn publishing_handler(
    fixture: &Fixture,
    topology: &rostfrei_nats::MessagingTopology,
    receipts: mpsc::UnboundedSender<(String, bool)>,
    pause: bool,
) -> TestResult<Arc<PublishingHandler>> {
    let context = async_nats::jetstream::new(async_nats::connect(&fixture.url).await?);
    let adapter = rostfrei_nats::NatsMessagingAdapter::new(
        rostfrei_nats::NatsPublisher::new(context.clone(), topology.clone()),
        rostfrei_nats::NatsCommandResponseReader::new(context, topology.clone()),
    );
    let scope = fixture
        .event_store
        .application()
        .bounded_context(fixture.event_store.bounded_context().as_str())?;
    Ok(Arc::new(PublishingHandler {
        bus: rostfrei::IntegrationEventBus::new(scope, Arc::new(adapter)),
        receipts,
        pause,
        published: Notify::new(),
    }))
}

#[tokio::test]
async fn takeover_after_puback_reuses_message_identity_and_deduplicates_on_the_bus()
-> TestResult<()> {
    let fixture = Fixture::new(Duration::from_millis(800), Duration::from_millis(500)).await?;
    let messaging =
        rostfrei_nats::ApplicationMessagingConfig::new(fixture.event_store.application())?
            .with_max_bytes(64 * 1024 * 1024)?;
    rostfrei_nats::provision_application_messaging(&fixture.context, &messaging).await?;
    let source = stream("publication")?;
    fixture
        .store
        .append(
            &source,
            ExpectedVersion::NoStream,
            batch(&source, "publication", &["one", "two"])?.with_correlation_id(
                rostfrei_messaging_core::CorrelationId::new("publication-test")?,
            ),
        )
        .await?;

    let (sender, mut receipts) = mpsc::unbounded_channel();
    let old_handler =
        publishing_handler(&fixture, messaging.topology(), sender.clone(), true).await?;
    let old_worker = fixture.worker(old_handler.clone()).await?;
    let (_stop, stopping) = watch::channel(false);
    let old = tokio::spawn(old_worker.run_until_shutdown(stopping));
    timeout(Duration::from_secs(3), old_handler.published.notified()).await?;
    let original = timeout(Duration::from_secs(3), receipts.recv())
        .await?
        .ok_or("missing original PubAck")?;
    assert!(!original.1);
    assert_eq!(fixture.info().await?.ack_floor.stream_sequence, 0);
    old.abort();
    assert!(old.await.is_err());

    let worker = fixture
        .worker(publishing_handler(&fixture, messaging.topology(), sender, false).await?)
        .await?;
    let (stop, stopping) = watch::channel(false);
    let task = tokio::spawn(worker.run_until_shutdown(stopping));
    let replay = timeout(Duration::from_secs(3), receipts.recv())
        .await?
        .ok_or("missing replay PubAck")?;
    assert_eq!(replay.0, original.0);
    assert!(
        replay.1,
        "JetStream must return a duplicate PubAck for the same source event"
    );
    let tail = timeout(Duration::from_secs(3), receipts.recv())
        .await?
        .ok_or("missing tail PubAck")?;
    assert_ne!(tail.0, original.0);
    assert!(!tail.1);
    fixture.drain().await?;
    let mut output = fixture
        .context
        .get_stream(messaging.topology().integration_event_stream().as_str())
        .await?;
    assert_eq!(output.info().await?.state.messages, 2);
    let first: rostfrei_messaging_core::IntegrationEventEnvelope<TestEvent> =
        serde_json::from_slice(&output.get_raw_message(1).await?.payload)?;
    let second: rostfrei_messaging_core::IntegrationEventEnvelope<TestEvent> =
        serde_json::from_slice(&output.get_raw_message(2).await?.payload)?;
    assert_eq!(first.payload().value, "one");
    assert_eq!(second.payload().value, "two");
    assert!(receipts.try_recv().is_err());
    stop.send(true)?;
    timeout(Duration::from_secs(3), task).await???;
    for stream in messaging.streams() {
        fixture
            .context
            .delete_stream(stream.name().as_str())
            .await?;
    }
    fixture.cleanup().await
}
