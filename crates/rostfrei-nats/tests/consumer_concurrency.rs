//! Independent connections share durable policy while enforcing local handler bounds.
#![allow(clippy::panic_in_result_fn, reason = "integration contract assertions")]

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CommandAddress, CommandPublisher, ConsumeError,
    ConsumerConfig, DeliveryDisposition, MessageConsumerFactory, MessageDelivery, MessageHandler,
    MessageId, OutboundMessage,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, MessagingTopology, NatsConnection, NatsConnectionConfig, connect,
    provision_application_messaging, provision_durable_consumer,
};
use tokio::{
    sync::{Mutex, mpsc, watch},
    task::JoinSet,
    time::{sleep, timeout},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const WAIT: Duration = Duration::from_secs(10);
const MESSAGES: usize = 96;
static FIXTURE_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    connections: [NatsConnection; 2],
    topology: MessagingTopology,
    context: BoundedContext,
}

impl Fixture {
    async fn new() -> TestResult<Self> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let application =
            ApplicationName::new(format!("local-concurrency-{}-{id}", std::process::id()))?;
        let context = application.bounded_context("workers")?;
        let messaging =
            ApplicationMessagingConfig::new(&application)?.with_max_bytes(4 * 1024 * 1024)?;
        let url = rostfrei_testing::integration::nats_url()?;
        let first = connect(&NatsConnectionConfig::new("worker-one", url.clone())).await?;
        let second = connect(&NatsConnectionConfig::new("worker-two", url)).await?;
        provision_application_messaging(first.jetstream(), &messaging).await?;
        Ok(Self {
            connections: [first, second],
            topology: messaging.topology().clone(),
            context,
        })
    }

    fn config(&self, local: usize, shared: usize) -> TestResult<ConsumerConfig<CommandAddress>> {
        Ok(ConsumerConfig::new(
            self.context.consumer_name("commands", 1)?,
            self.context.durable_name("commands", 1)?,
            self.context.command_address("execute")?,
            Duration::from_secs(45),
            Duration::from_secs(30),
            local,
            5,
        )?
        .with_shared_pending_limit(shared)?)
    }

    async fn cleanup(self) -> TestResult {
        let [first, second] = self.connections;
        for stream in [
            self.topology.command_stream(),
            self.topology.command_response_stream(),
            self.topology.integration_event_stream(),
            self.topology.quarantine_stream(),
        ] {
            first.jetstream().delete_stream(stream.as_str()).await?;
        }
        first.drain().await?;
        second.drain().await?;
        Ok(())
    }
}

#[derive(Default)]
struct Counts {
    active: AtomicUsize,
    peak: AtomicUsize,
    finished: AtomicUsize,
}

struct Active<'a>(&'a Counts);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Handler {
    worker: usize,
    counts: Arc<Counts>,
    seen: Arc<Mutex<HashSet<MessageId>>>,
    duplicates: Arc<AtomicUsize>,
    redeliveries: Arc<AtomicUsize>,
    entered: mpsc::UnboundedSender<usize>,
    release: watch::Receiver<bool>,
}

#[async_trait]
impl MessageHandler<CommandAddress> for Handler {
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        let active = self.counts.active.fetch_add(1, Ordering::SeqCst) + 1;
        let _active = Active(&self.counts);
        self.counts.peak.fetch_max(active, Ordering::SeqCst);
        if !self.seen.lock().await.insert(delivery.message_id().clone()) {
            self.duplicates.fetch_add(1, Ordering::SeqCst);
        }
        if delivery.attempt() != 1 {
            self.redeliveries.fetch_add(1, Ordering::SeqCst);
        }
        let _ = self.entered.send(self.worker);
        let mut release = self.release.clone();
        if release.wait_for(|released| *released).await.is_err() {
            return DeliveryDisposition::Terminate;
        }
        self.counts.finished.fetch_add(1, Ordering::SeqCst);
        DeliveryDisposition::Acknowledge
    }
}

async fn expect_entries(
    entries: &mut mpsc::UnboundedReceiver<usize>,
    jobs: &mut JoinSet<Result<(), ConsumeError>>,
    worker: usize,
    count: usize,
) -> TestResult {
    timeout(WAIT, async {
        for _ in 0..count {
            tokio::select! {
                entry = entries.recv() => assert_eq!(entry, Some(worker)),
                ended = jobs.join_next() => return Err(format!("worker exited: {ended:?}").into()),
            }
        }
        Ok(())
    })
    .await?
}

#[allow(
    clippy::too_many_lines,
    reason = "one gated two-connection scaling scenario"
)]
async fn exercise(local: [usize; 2], shared: usize, second_can_fill: bool) -> TestResult {
    let fixture = Fixture::new().await?;
    let [first, second] = &fixture.connections;
    let [first_limit, second_limit] = local;
    let configs = [
        fixture.config(first_limit, shared)?,
        fixture.config(second_limit, shared)?,
    ];
    let [first_config, second_config] = &configs;
    // Exercise the explicit operator update from the old coupled budget as well.
    let original = fixture.config(first_limit, first_limit)?;
    let before =
        provision_durable_consumer(first.jetstream(), &fixture.topology, &original).await?;
    let after =
        provision_durable_consumer(first.jetstream(), &fixture.topology, first_config).await?;
    assert_eq!(before.created, after.created);
    assert_eq!(after.config.max_ack_pending, i64::try_from(shared)?);
    let reprovisioned =
        provision_durable_consumer(second.jetstream(), &fixture.topology, second_config).await?;
    assert_eq!(after.config, reprovisioned.config);

    let publisher = first.publisher(fixture.topology.clone());
    for index in 0..MESSAGES {
        publisher
            .publish_command(OutboundMessage::new(
                first_config.address().clone(),
                MessageId::new(format!("command-{index}"))?,
                b"{}".to_vec(),
            )?)
            .await?;
    }

    let counts = [Arc::new(Counts::default()), Arc::new(Counts::default())];
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let duplicates = Arc::new(AtomicUsize::new(0));
    let redeliveries = Arc::new(AtomicUsize::new(0));
    let (entered, mut entries) = mpsc::unbounded_channel();
    let (release, gate) = watch::channel(false);
    let mut jobs = JoinSet::new();
    for (worker, ((connection, config), counts)) in fixture
        .connections
        .iter()
        .zip(&configs)
        .zip(&counts)
        .enumerate()
    {
        let factory = connection.consumer_factory(fixture.topology.clone());
        factory.verify_consumer(config).await?;
        let consumer = factory.create(config.clone())?;
        let handler = Arc::new(Handler {
            worker,
            counts: counts.clone(),
            seen: seen.clone(),
            duplicates: duplicates.clone(),
            redeliveries: redeliveries.clone(),
            entered: entered.clone(),
            release: gate.clone(),
        });
        jobs.spawn(async move { consumer.run(handler).await });
        if worker == 0 || second_can_fill {
            expect_entries(&mut entries, &mut jobs, worker, config.concurrency()).await?;
        }
    }
    // Backlog is ready and handlers cannot finish: extra entry would violate a limit.
    tokio::select! {
        entry = entries.recv() => return Err(format!("handler exceeded the admitted bound: {entry:?}").into()),
        ended = jobs.join_next() => return Err(format!("worker exited: {ended:?}").into()),
        () = sleep(Duration::from_millis(200)) => {},
    }
    let [first_counts, second_counts] = &counts;
    assert_eq!(first_counts.active.load(Ordering::SeqCst), first_limit);
    assert_eq!(
        second_counts.active.load(Ordering::SeqCst),
        if second_can_fill { second_limit } else { 0 }
    );
    release.send(true)?;

    let stream = first
        .jetstream()
        .get_stream(fixture.topology.command_stream().as_str())
        .await?;
    let mut inspector: async_nats::jetstream::consumer::PullConsumer = stream
        .get_consumer(first_config.durable_name().as_str())
        .await?;
    timeout(WAIT, async {
        loop {
            let info = inspector.info().await?;
            if info.num_pending == 0 && info.num_ack_pending == 0 {
                break;
            }
            tokio::select! {
                ended = jobs.join_next() => return Err(format!("worker exited: {ended:?}").into()),
                () = sleep(Duration::from_millis(10)) => {},
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    assert_eq!(
        counts
            .iter()
            .map(|c| c.finished.load(Ordering::SeqCst))
            .sum::<usize>(),
        MESSAGES
    );
    assert_eq!(seen.lock().await.len(), MESSAGES);
    assert_eq!(duplicates.load(Ordering::SeqCst), 0);
    assert_eq!(redeliveries.load(Ordering::SeqCst), 0);
    for (counts, limit) in counts.iter().zip(local) {
        assert!(counts.peak.load(Ordering::SeqCst) <= limit);
        assert_eq!(counts.active.load(Ordering::SeqCst), 0);
    }
    jobs.abort_all();
    while let Some(result) = jobs.join_next().await {
        assert!(result.is_err_and(|error| error.is_cancelled()));
    }
    fixture.cleanup().await
}

#[tokio::test]
async fn two_workers_can_each_run_sixteen_handlers() -> TestResult {
    Box::pin(exercise([16, 16], 128, true)).await
}

#[tokio::test]
async fn mixed_local_limits_share_one_unchanged_durable_policy() -> TestResult {
    Box::pin(exercise([8, 16], 128, true)).await
}

#[tokio::test]
async fn shared_budget_remains_a_global_ceiling() -> TestResult {
    Box::pin(exercise([16, 16], 16, false)).await
}
