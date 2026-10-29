#![allow(clippy::expect_used, clippy::panic_in_result_fn)]

use super::*;
use async_nats::jetstream;
use futures_util::StreamExt as _;
use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};
use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

struct Cluster {
    children: Vec<Child>,
    urls: Vec<String>,
    directory: tempfile::TempDir,
}

impl Cluster {
    fn start() -> TestResult<Self> {
        let mut cluster = Self {
            children: Vec::new(),
            urls: Vec::new(),
            directory: tempfile::tempdir()?,
        };
        let ports = (0..3)
            .map(|_| {
                Ok((
                    TcpListener::bind("127.0.0.1:0")?,
                    TcpListener::bind("127.0.0.1:0")?,
                ))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let routes = ports
            .iter()
            .map(|(_, route)| Ok(format!("nats://{}", route.local_addr()?)))
            .collect::<TestResult<Vec<_>>>()?
            .join(",");
        let binary =
            std::env::var_os("ROSTFREI_NATS_SERVER").unwrap_or_else(|| "nats-server".into());
        for (index, (client, route)) in ports.into_iter().enumerate() {
            let client_address = client.local_addr()?;
            let route_address = route.local_addr()?;
            let config = cluster.directory.path().join(format!("node-{index}.conf"));
            let storage = cluster.directory.path().join(format!("node-{index}"));
            std::fs::write(
                &config,
                format!(
                    "server_name: history-{index}\nlisten: {client_address}\n\
                     jetstream {{ store_dir: \"{}\", max_file_store: 256MB, max_memory_store: 64MB }}\n\
                     cluster {{ name: history, listen: {route_address}, routes: [{routes}] }}\n",
                    storage.display(),
                ),
            )?;
            let log =
                std::fs::File::create(cluster.directory.path().join(format!("node-{index}.log")))?;
            drop(client);
            drop(route);
            cluster.children.push(
                Command::new(&binary)
                    .arg("-c")
                    .arg(config)
                    .stdout(Stdio::from(log.try_clone()?))
                    .stderr(Stdio::from(log))
                    .spawn()?,
            );
            cluster.urls.push(format!("nats://{client_address}"));
        }
        Ok(cluster)
    }

    async fn fixture(&self) -> TestResult<(jetstream::Context, NatsEventStore)> {
        let (bounded_context, stream_name) = unique_names("history-cluster")?;
        let config = NatsEventStoreConfig::new(&bounded_context, stream_name)?
            .with_replicas(1)?
            .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
        let context = timeout(Duration::from_secs(15), async {
            loop {
                if let Ok(client) =
                    async_nats::connect(self.urls.first().expect("first node")).await
                {
                    let context = jetstream::new(client);
                    if provision_event_store(&context, &config).await.is_ok() {
                        break context;
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await?;
        let stream = context.get_stream(config.stream_name()).await?;
        let leader = stream
            .cached_info()
            .cluster
            .as_ref()
            .expect("clustered stream")
            .leader
            .as_deref()
            .ok_or("stream has no leader")?;
        for url in &self.urls {
            let client = async_nats::ConnectOptions::new()
                .ignore_discovered_servers()
                .connect(url)
                .await?;
            if client.server_info().server_name != leader {
                let context = jetstream::new(client);
                let store = NatsEventStore::connect(context.clone(), config).await?;
                return Ok((context, store));
            }
        }
        Err("no remote client broker found".into())
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
        if std::thread::panicking() {
            for index in 0..3 {
                if let Ok(log) =
                    std::fs::read_to_string(self.directory.path().join(format!("node-{index}.log")))
                {
                    eprintln!("cluster node {index}:\n{log}");
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Fault {
    Initial(usize),
    Always,
    AfterDelivery,
    AlwaysAfterDelivery,
    DelayedRecreation,
}

#[derive(Debug)]
struct Fetch {
    consumer: String,
    created: String,
    delivered: u64,
    batch: u64,
    failed: bool,
}

struct FetchRelay {
    task: tokio::task::JoinHandle<TestResult<()>>,
    fetches: Arc<Mutex<Vec<Fetch>>>,
    context: jetstream::Context,
    delayed_create: oneshot::Receiver<(String, async_nats::Message)>,
    missing_delete: oneshot::Receiver<()>,
    deletes: Arc<AtomicU64>,
}

impl FetchRelay {
    async fn start(context: &jetstream::Context, fault: Fault) -> TestResult<Self> {
        let client = context.client().clone();
        let prefix = client.new_inbox();
        let mut requests = client.subscribe(format!("{prefix}.>")).await?;
        client.flush().await?;
        let relay_context = jetstream::with_prefix(client.clone(), &prefix);
        let context = context.clone();
        let fetches = Arc::new(Mutex::new(Vec::<Fetch>::new()));
        let recorded = Arc::clone(&fetches);
        let (delayed_tx, delayed_create) = oneshot::channel();
        let (missing_tx, missing_delete) = oneshot::channel();
        let deletes = Arc::new(AtomicU64::new(0));
        let recorded_deletes = Arc::clone(&deletes);
        let task = tokio::spawn(async move {
            let mut delayed_tx = Some(delayed_tx);
            let mut missing_tx = Some(missing_tx);
            let mut creates = 0_usize;
            while let Some(request) = requests.next().await {
                let suffix = request
                    .subject
                    .strip_prefix(&prefix)
                    .ok_or("wrong API prefix")?;
                let mut destination = format!("$JS.API{suffix}");
                if suffix.starts_with(".CONSUMER.DELETE.") {
                    recorded_deletes.fetch_add(1, Ordering::Relaxed);
                }
                if matches!(fault, Fault::DelayedRecreation) {
                    if suffix.starts_with(".CONSUMER.CREATE.") {
                        creates = creates.checked_add(1).ok_or("create count overflow")?;
                        if creates == 2 {
                            delayed_tx
                                .take()
                                .ok_or("recreation already held")?
                                .send((destination, request))
                                .map_err(|_| "delayed creation receiver dropped")?;
                            continue;
                        }
                    }
                    if suffix.starts_with(".CONSUMER.DELETE.") {
                        let response = client.request(destination, request.payload).await?;
                        let body: serde_json::Value = serde_json::from_slice(&response.payload)?;
                        let missing = body
                            .get("error")
                            .and_then(|error| error.get("err_code"))
                            .and_then(serde_json::Value::as_u64)
                            == Some(10014);
                        client
                            .publish(
                                request.reply.ok_or("delete has no reply")?,
                                response.payload,
                            )
                            .await?;
                        if missing && let Some(sender) = missing_tx.take() {
                            sender.send(()).map_err(|()| "delete receiver dropped")?;
                        }
                        continue;
                    }
                }
                if let Some(cursor) = suffix.strip_prefix(".CONSUMER.MSG.NEXT.")
                    && fail_fetch(&context, cursor, &request, fault, &recorded).await?
                {
                    destination = format!("unrouted{prefix}");
                }
                client
                    .publish_with_reply(
                        destination,
                        request.reply.ok_or("API request has no reply")?,
                        request.payload,
                    )
                    .await?;
            }
            Ok(())
        });
        Ok(Self {
            task,
            fetches,
            context: relay_context,
            delayed_create,
            missing_delete,
            deletes,
        })
    }

    fn assert_recreated_cursor(&self) {
        assert!(!self.task.is_finished(), "fetch relay failed");
        let fetches = self.fetches.lock().expect("fetch log");
        let first = fetches.first().expect("at least one fetch");
        assert_eq!(first.delivered, 0);
        assert!(fetches.iter().all(|fetch| fetch.consumer == first.consumer));
        assert!(fetches.iter().all(|fetch| fetch.batch == 500));
        for pair in fetches.windows(2) {
            if let [previous, next] = pair
                && previous.failed
            {
                assert_ne!(previous.created, next.created);
                assert_eq!(next.delivered, 0);
            }
        }
        eprintln!(
            "remote replay fetches ({}): {:?}",
            fetches.len(),
            fetches.iter().take(10).collect::<Vec<_>>()
        );
        drop(fetches);
    }
}

async fn fail_fetch(
    context: &jetstream::Context,
    cursor: &str,
    request: &async_nats::Message,
    fault: Fault,
    recorded: &Mutex<Vec<Fetch>>,
) -> TestResult<bool> {
    let (stream, consumer) = cursor.split_once('.').ok_or("invalid fetch subject")?;
    let consumer: jetstream::consumer::PullConsumer =
        context.get_consumer_from_stream(consumer, stream).await?;
    let info = consumer.cached_info();
    let leader = info
        .cluster
        .as_ref()
        .ok_or("consumer is not clustered")?
        .leader
        .as_deref()
        .ok_or("consumer has no leader")?;
    assert_ne!(leader, context.client().server_info().server_name);
    assert!(info.config.durable_name.is_none());
    assert_eq!(info.config.name.as_deref(), Some(info.name.as_str()));
    let delivered = info.delivered.consumer_sequence;
    let payload: serde_json::Value = serde_json::from_slice(&request.payload)?;
    let batch = payload
        .get("batch")
        .and_then(serde_json::Value::as_u64)
        .ok_or("missing fetch batch")?;
    let mut fetches = recorded.lock().expect("fetch log");
    let failed = match fault {
        Fault::Initial(count) => fetches.len() < count,
        Fault::Always | Fault::DelayedRecreation => true,
        Fault::AfterDelivery => delivered > 0 && !fetches.iter().any(|fetch| fetch.failed),
        Fault::AlwaysAfterDelivery => delivered > 0 || fetches.iter().any(|fetch| fetch.failed),
    };
    fetches.push(Fetch {
        consumer: info.name.clone(),
        created: info.created.to_string(),
        delivered,
        batch,
        failed,
    });
    drop(fetches);
    Ok(failed)
}

impl Drop for FetchRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn assert_clean(context: &jetstream::Context, store: &NatsEventStore) -> TestResult<()> {
    let stream = context.get_stream(store.config().stream_name()).await?;
    assert_eq!(stream.cached_info().state.consumer_count, 0);
    Ok(())
}

async fn assert_eventually_clean(
    context: &jetstream::Context,
    store: &NatsEventStore,
) -> TestResult<()> {
    let started = Instant::now();
    timeout(Duration::from_secs(45), async {
        loop {
            let stream = context.get_stream(store.config().stream_name()).await?;
            if stream.cached_info().state.consumer_count == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    eprintln!("replay cleanup observed after {:?}", started.elapsed());
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn initial_no_responders_recovers_remote_history_in_order() -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("remote-history")?;
    let expected = history_reads::seed_history(&store, &aggregate).await?;
    let relay = FetchRelay::start(&context, Fault::Initial(3)).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;

    let actual = timeout(Duration::from_secs(7), reader.load(&aggregate)).await??;
    assert_eq!(
        actual, expected,
        "all 700 events must retain their order and identity"
    );
    relay.assert_recreated_cursor();
    let failures = relay
        .fetches
        .lock()
        .expect("fetch log")
        .iter()
        .filter(|f| f.failed)
        .count();
    assert_eq!(failures, 3);
    assert!(relay.fetches.lock().expect("fetch log").len() <= 10);
    assert_clean(&context, &store).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn initial_no_responders_stops_at_read_deadline() -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("unavailable-history")?;
    store
        .append(
            &aggregate,
            ExpectedVersion::NoStream,
            batch(&aggregate, "seed", "seed", &[b"one"])?,
        )
        .await?;
    let relay = FetchRelay::start(&context, Fault::Always).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;

    let started = Instant::now();
    let result = timeout(Duration::from_secs(7), reader.load(&aggregate)).await?;
    let elapsed = started.elapsed();
    eprintln!("initial persistent failure returned after {elapsed:?}");
    assert!(matches!(result, Err(error) if error.kind() == EventStoreErrorKind::Unavailable));
    assert!(
        elapsed >= Duration::from_secs(5),
        "stopped retrying after {elapsed:?}"
    );
    relay.assert_recreated_cursor();
    let count = relay.fetches.lock().expect("fetch log").len();
    assert!(count > 1, "native recovery must retry: {count}");
    assert_eventually_clean(&context, &store).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn transient_no_responders_after_delivery_recovers_batched_history() -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("partial-history")?;
    let expected = history_reads::seed_history(&store, &aggregate).await?;
    let relay = FetchRelay::start(&context, Fault::AfterDelivery).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;

    let result = timeout(Duration::from_secs(7), reader.load(&aggregate)).await?;
    relay.assert_recreated_cursor();
    assert_clean(&context, &store).await?;
    let actual = result.expect("transient 503 after progress must recover all 700 events");
    assert_eq!(
        actual, expected,
        "recovery must not duplicate or truncate history"
    );
    {
        let fetches = relay.fetches.lock().expect("fetch log");
        assert!(
            (2..=10).contains(&fetches.len()),
            "700 events must use batched pulls: {fetches:?}"
        );
        assert!(fetches.iter().all(|fetch| (2..=500).contains(&fetch.batch)));
        let failures = fetches
            .iter()
            .filter(|fetch| fetch.failed)
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 1);
        assert!(failures.first().expect("failed fetch").delivered > 0);
        assert!(!fetches.last().expect("recovered fetch").failed);
        drop(fetches);
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn persistent_no_responders_after_delivery_stops_at_read_deadline() -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("persistently-unavailable-history")?;
    history_reads::seed_history(&store, &aggregate).await?;
    let relay = FetchRelay::start(&context, Fault::AlwaysAfterDelivery).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;

    let started = Instant::now();
    let result = timeout(Duration::from_secs(7), reader.load(&aggregate)).await?;
    let elapsed = started.elapsed();
    eprintln!("persistent failure after delivery returned after {elapsed:?}");
    relay.assert_recreated_cursor();
    assert!(matches!(result, Err(error) if error.kind() == EventStoreErrorKind::Unavailable));
    assert!(
        elapsed >= Duration::from_secs(5),
        "persistent 503 after progress stopped recovery before the read deadline: {elapsed:?}"
    );
    {
        let fetches = relay.fetches.lock().expect("fetch log");
        assert!(
            fetches
                .iter()
                .any(|fetch| fetch.failed && fetch.delivered > 0)
        );
        assert!(fetches.iter().filter(|fetch| fetch.failed).count() > 1);
        drop(fetches);
    }
    assert_eventually_clean(&context, &store).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn cancelled_recovery_cleans_up_the_recreated_cursor() -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("cancelled-recovery")?;
    history_reads::seed_history(&store, &aggregate).await?;
    let relay = FetchRelay::start(&context, Fault::AlwaysAfterDelivery).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;
    let task = tokio::spawn(async move { reader.load(&aggregate).await });

    timeout(Duration::from_secs(5), async {
        loop {
            if relay.fetches.lock().expect("fetch log").len() >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    relay.assert_recreated_cursor();
    task.abort();
    assert!(task.await.expect_err("cancelled replay").is_cancelled());
    assert_eventually_clean(&context, &store).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn timed_out_read_cleans_up_late_native_recreation() -> TestResult<()> {
    late_native_recreation(false).await
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn cancelled_read_cleans_up_late_native_recreation() -> TestResult<()> {
    late_native_recreation(true).await
}

async fn late_native_recreation(cancel: bool) -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("late-native-recreation")?;
    store
        .append(
            &aggregate,
            ExpectedVersion::NoStream,
            batch(&aggregate, "seed", "seed", &[b"one"])?,
        )
        .await?;
    let mut relay = FetchRelay::start(&context, Fault::DelayedRecreation).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;
    let started = Instant::now();
    let task = tokio::spawn(async move { reader.load(&aggregate).await });
    let (destination, request) =
        timeout(Duration::from_secs(5), &mut relay.delayed_create).await??;
    if cancel {
        task.abort();
        assert!(task.await.expect_err("cancelled replay").is_cancelled());
    } else {
        let result = timeout(Duration::from_secs(7), task).await??;
        assert!(matches!(result, Err(error) if error.kind() == EventStoreErrorKind::Unavailable));
        assert!(started.elapsed() >= Duration::from_secs(5));
    }
    timeout(Duration::from_secs(5), &mut relay.missing_delete).await??;
    assert_clean(&context, &store).await?;
    let deletes = relay.deletes.load(Ordering::Relaxed);
    let response = context
        .client()
        .request(destination, request.payload)
        .await?;
    let created: jetstream::consumer::Info = serde_json::from_slice(&response.payload)?;
    context
        .client()
        .publish(
            request.reply.ok_or("held create has no reply")?,
            response.payload,
        )
        .await?;
    let consumer: jetstream::consumer::PullConsumer = context
        .get_consumer_from_stream(&created.name, store.config().stream_name())
        .await?;
    assert_eq!(consumer.cached_info().name, created.name);
    {
        let fetches = relay.fetches.lock().expect("fetch log");
        let initial = fetches.first().expect("initial failed fetch");
        assert_eq!(created.name, initial.consumer);
        assert_ne!(created.created.to_string(), initial.created);
        drop(fetches);
    }
    assert_eq!(
        consumer.cached_info().config.inactive_threshold,
        Duration::from_secs(30)
    );
    assert!(consumer.cached_info().config.durable_name.is_none());
    let stream = context.get_stream(store.config().stream_name()).await?;
    assert_eq!(stream.cached_info().state.consumer_count, 1);
    assert!(!relay.task.is_finished(), "fetch relay failed");
    eprintln!(
        "late native CREATE acknowledged after read completion (cancel={cancel}); cleanup DELETE returned 404, consumer INFO succeeds, stream consumer count is 1"
    );
    assert_eventually_clean(&context, &store).await?;
    assert!(matches!(
        consumer.get_info().await,
        Err(error) if error.kind() == jetstream::context::ConsumerInfoErrorKind::NotFound
    ));
    assert_eq!(
        relay.deletes.load(Ordering::Relaxed),
        deletes,
        "native inactivity must remove the late consumer without another DELETE"
    );
    assert!(!relay.task.is_finished(), "fetch relay failed");
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker cluster"]
async fn recovered_replay_still_rejects_corrupt_history() -> TestResult<()> {
    let cluster = Cluster::start()?;
    let (context, store) = cluster.fixture().await?;
    let aggregate = stream("corrupt-history")?;
    context
        .publish(
            store.config().event_subject(
                aggregate.aggregate_type().as_str(),
                aggregate.aggregate_id().as_str(),
                None,
                None,
            ),
            b"malformed event".to_vec().into(),
        )
        .await?
        .await?;
    let relay = FetchRelay::start(&context, Fault::Initial(3)).await?;
    let reader = NatsEventStore::connect(relay.context.clone(), store.config().clone()).await?;

    let result = timeout(Duration::from_secs(7), reader.load(&aggregate)).await?;
    assert!(matches!(result, Err(error) if error.kind() == EventStoreErrorKind::CorruptHistory));
    relay.assert_recreated_cursor();
    assert_clean(&context, &store).await?;
    Ok(())
}
