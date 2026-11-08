//! Native R3/file qualification; set `ROSTFREI_NATS_SERVER` and run with `--ignored`.
#![allow(clippy::panic_in_result_fn)]

mod cluster_support;

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use async_nats::jetstream::{self, consumer, stream::StorageType};
use cluster_support::Cluster;
use futures_util::{StreamExt, TryStreamExt};
use rostfrei_core::{
    AggregateId, AggregateType, CommandCompletion, CommandExecutionMetadata, ContentFingerprint,
    CorrelationHistory, CorrelationRead, CorrelationRef, CorrelationSourceKind, DomainEventRef,
    EventBatch, EventStore, EventStoreErrorKind, EventTransaction, ExpectedVersion,
    JsonRejectionCodec, NewEvent, OperationId, RecordedEvent, RejectionCodec, StreamId,
    TransactionParticipant, derive_commit_id, derive_event_id,
};
use rostfrei_messaging_core::{ApplicationName, CausationId, CorrelationId, MessageTimestamp};
use rostfrei_nats::{
    ApplicationMessagingConfig, MessagingTopology, NatsCorrelationHistory, NatsEventStore,
    NatsEventStoreConfig, provision_event_store, provision_stream,
};
use tokio::{sync::oneshot, time::timeout};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const WAIT: Duration = Duration::from_secs(60);
const READ_WAIT: Duration = Duration::from_secs(20);
const REF_LIMIT: usize = 7;
const DURABLE: &str = "application-progress";
const REQUESTED: [&str; 4] = ["flow-a", "flow-b", "decisions-only", "absent"];

impl Cluster {
    async fn connect(&self) -> TestResult<jetstream::Context> {
        Ok(timeout(WAIT, async {
            loop {
                if let Ok(client) = async_nats::connect(self.urls.clone()).await {
                    break jetstream::new(client);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await?)
    }
}

struct ResumeOnDrop<'a> {
    cluster: &'a Cluster,
    node: usize,
}

impl Drop for ResumeOnDrop<'_> {
    fn drop(&mut self) {
        let _ = self.cluster.pause(self.node, false);
    }
}

struct Fixture {
    js: jetstream::Context,
    store: NatsEventStore,
    topology: MessagingTopology,
    reader: NatsCorrelationHistory,
    created: time::OffsetDateTime,
}

impl Fixture {
    async fn new(cluster: &Cluster) -> TestResult<Self> {
        let js = cluster.connect().await?;
        let application = ApplicationName::new("receipt-cluster")?;
        let messaging = ApplicationMessagingConfig::new(&application)?
            .with_replicas(3)?
            .with_max_bytes(16 * 1024 * 1024)?;
        let config =
            NatsEventStoreConfig::for_bounded_context(&application.bounded_context("sales")?)?
                .with_fresh_split_outcome_epoch("receipt-cluster")?
                .with_replicas(3)?
                .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?;
        timeout(WAIT, async {
            loop {
                if provision_event_store(&js, &config).await.is_ok()
                    && provision_stream(&js, messaging.integration_events())
                        .await
                        .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?;
        let journal = js.get_stream(config.stream_name()).await?;
        let leader = journal
            .cached_info()
            .cluster
            .as_ref()
            .and_then(|cluster| cluster.leader.as_deref())
            .ok_or("journal has no leader")?;
        // Exercise remote leader-served MSG.GET, rather than placing the reader
        // on the stream leader by accident. Native discovery/reconnect stays on.
        let mut remote = None;
        for url in &cluster.urls {
            let client = async_nats::connect(url).await?;
            if client.server_info().server_name != leader {
                remote = Some(jetstream::new(client));
                break;
            }
        }
        let js = remote.ok_or("no follower connection")?;
        let store = NatsEventStore::connect(js.clone(), config).await?;
        let reader = NatsCorrelationHistory::connect(
            js.clone(),
            vec![store.config().clone()],
            messaging.topology(),
        )
        .await?;
        let fixture = Self {
            js,
            store,
            topology: messaging.topology().clone(),
            reader,
            created: journal.cached_info().created,
        };
        fixture.wait_replicas(3, None).await?;
        Ok(fixture)
    }

    async fn journal(&self) -> TestResult<jetstream::stream::Stream> {
        Ok(self
            .js
            .get_stream(self.store.config().stream_name())
            .await?)
    }

    async fn wait_replicas(&self, live: usize, excluded: Option<&str>) -> TestResult<String> {
        timeout(WAIT, async {
            loop {
                if let Ok(stream) = self.journal().await {
                    let info = stream.cached_info();
                    assert_eq!(info.created, self.created, "restart replaced the journal");
                    assert_eq!(info.config.num_replicas, 3);
                    assert_eq!(info.config.storage, StorageType::File);
                    if let Some(cluster) = &info.cluster
                        && let Some(leader) = &cluster.leader
                        && excluded != Some(leader.as_str())
                        && cluster.replicas.len() == 2
                        && cluster
                            .replicas
                            .iter()
                            .filter(|peer| peer.current && !peer.offline)
                            .count()
                            >= live.saturating_sub(1)
                    {
                        return Ok(leader.clone());
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?
    }

    async fn assert_consumers(&self) -> TestResult {
        let journal = self.journal().await?;
        let names: BTreeSet<_> = journal.consumer_names().try_collect().await?;
        let integration = self
            .js
            .get_stream(self.topology.integration_event_stream().as_str())
            .await?;
        if names != BTreeSet::from([DURABLE.to_owned()])
            || integration.cached_info().state.consumer_count != 0
        {
            return Err(format!(
                "inspection left consumers: domain={names:?}, integration={}",
                integration.cached_info().state.consumer_count
            )
            .into());
        }
        Ok(())
    }

    async fn wait_consumers(&self) -> TestResult {
        // Only failed reads may rely on native inactivity expiry. Successful
        // reads must have awaited deletion when they return.
        timeout(Duration::from_secs(45), async {
            loop {
                if self.assert_consumers().await.is_ok() {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?
    }
}

struct Workload {
    aggregates: Vec<StreamId>,
    events: Vec<RecordedEvent>,
    next: u32,
}

impl Workload {
    fn new() -> TestResult<Self> {
        Ok(Self {
            aggregates: ["order", "invoice", "policy", "absent-guard"]
                .into_iter()
                .map(|id| {
                    Ok(StreamId::new(
                        AggregateType::new(id)?,
                        AggregateId::new("one")?,
                    ))
                })
                .collect::<TestResult<_>>()?,
            events: Vec::new(),
            next: 0,
        })
    }

    async fn write(&mut self, store: &NatsEventStore) -> TestResult {
        let operation = OperationId::new(format!("command-{}", self.next))?;
        let fingerprint = ContentFingerprint::digest(operation.as_str());
        let correlation = CorrelationId::new(match self.next % 4 {
            0 if self.next != 0 => "flow-a",
            2 => "flow-b",
            3 => "decisions-only",
            _ => "noise",
        })?;
        let cause = CausationId::new(format!("cause-{}", self.next))?;
        let counts = if self.next == 0 {
            [0, 0, 1, 0] // Establish the nonzero read-guard checkpoint.
        } else if self.next % 4 == 3 {
            [0, 0, 0, 0] // Acceptance/rejection with guards but no events.
        } else {
            [2, 1, 0, 0] // Multi-aggregate writes plus existing/absent guards.
        };
        let participants = self
            .aggregates
            .iter()
            .zip(counts)
            .map(|(stream, count)| {
                let expected = self
                    .events
                    .iter()
                    .rev()
                    .find(|event| event.stream_id() == stream)
                    .map_or(ExpectedVersion::NoStream, |event| {
                        ExpectedVersion::Exact(event.stream_version())
                    });
                let batch = if count == 0 {
                    None
                } else {
                    let commit = derive_commit_id(stream, &operation);
                    let events = (0..count)
                        .map(|ordinal| {
                            NewEvent::new(
                                derive_event_id(&commit, ordinal),
                                "changed",
                                1,
                                format!("{}:{ordinal}", operation.as_str()).into_bytes(),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Some(
                        EventBatch::new(commit, operation.clone(), fingerprint, events)?
                            .with_correlation_id(correlation.clone())
                            .with_causation_id(cause.clone()),
                    )
                };
                Ok(TransactionParticipant::new(stream.clone(), expected, batch))
            })
            .collect::<TestResult<_>>()?;
        let metadata = CommandExecutionMetadata::new(operation.clone(), fingerprint)
            .with_default_outcome_evidence::<()>();
        let rejection = if self.next % 8 == 7 {
            Some(JsonRejectionCodec::new("refused", 1).encode(&"unchanged".to_owned())?)
        } else {
            None
        };
        let transaction = EventTransaction::new(operation, fingerprint, participants)
            .with_bounded_context(store.config().bounded_context().clone())
            .with_correlation_id(correlation)
            .with_causation_id(cause)
            .with_completion(CommandCompletion::new(
                metadata.outcome_evidence().cloned(),
                rejection,
            ));
        // Consumer creation and outcome reconciliation can be transiently
        // unavailable after election. Retry for the bounded recovery window:
        // several immediate 503s do not mean that window has elapsed. Reuse the
        // identical operation and evidence after an uncertain write.
        let committed = timeout(Duration::from_secs(30), async {
            loop {
                match store.commit_outcome(transaction.clone()).await {
                    Ok(committed) => return Ok(committed),
                    Err(error) if error.kind() == EventStoreErrorKind::Unavailable => {
                        eprintln!(
                            "retrying {} after {error}",
                            transaction.operation_id().as_str()
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        })
        .await??;
        let events = committed.outcome().record().events();
        assert_eq!(
            events.len(),
            usize::try_from(counts.into_iter().sum::<u32>())?
        );
        self.events.extend(events);
        self.next = self.next.checked_add(1).ok_or("operation count overflow")?;
        Ok(())
    }
}

struct Sample {
    phase: &'static str,
    read: CorrelationRead,
    limit: Option<usize>,
}

fn requested() -> TestResult<Vec<CorrelationId>> {
    Ok(REQUESTED
        .into_iter()
        .map(CorrelationId::new)
        .collect::<Result<_, _>>()?)
}

async fn sample(fixture: &Fixture, phase: &'static str, limited: bool) -> TestResult<Sample> {
    let reader = if limited {
        fixture
            .reader
            .clone()
            .with_limits(REF_LIMIT, 32 * 1024 * 1024)?
    } else {
        fixture.reader.clone()
    };
    Ok(Sample {
        phase,
        read: timeout(READ_WAIT, reader.load_correlations(&requested()?)).await??,
        limit: limited.then_some(REF_LIMIT),
    })
}

async fn while_writing(
    fixture: &Fixture,
    workload: &mut Workload,
    phase: &'static str,
) -> TestResult<Vec<Sample>> {
    let before = fixture.journal().await?.cached_info().state.last_sequence;
    let (started, first_commit) = oneshot::channel();
    let (finished, read_done) = oneshot::channel();
    let writer = async {
        workload.write(&fixture.store).await?;
        started.send(()).map_err(|()| "reader dropped")?;
        for _ in 0..22 {
            workload.write(&fixture.store).await?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        read_done.await?;
        workload.write(&fixture.store).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let observation = async {
        first_commit.await?;
        let mut samples = Vec::new();
        let mut transient_errors = 0_u32;
        for limited in [false, true, false, false] {
            loop {
                match sample(fixture, phase, limited).await {
                    Ok(sample) => {
                        samples.push(sample);
                        break;
                    }
                    Err(error)
                        if transient_errors < 2
                            && error
                                .downcast_ref::<rostfrei_core::EventStoreError>()
                                .is_some_and(|error| {
                                    error.kind() == EventStoreErrorKind::Unavailable
                                }) =>
                    {
                        transient_errors = transient_errors.saturating_add(1);
                        eprintln!("bounded transient during {phase}: {error}");
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        finished.send(()).map_err(|()| "writer dropped")?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(samples)
    };
    let (committed, collected) = timeout(WAIT, async { tokio::join!(writer, observation) }).await?;
    committed?;
    let samples = collected?;
    let after = fixture.journal().await?.cached_info().state.last_sequence;
    for sample in &samples {
        let cutoff = domain_cutoff(fixture, &sample.read)?;
        assert!(
            before < cutoff && cutoff < after,
            "{phase}: no live-write cutoff"
        );
    }
    Ok(samples)
}

// Observe native immutable reads instead of a timer: fault an in-flight source
// read. There is no request relay or simulated broker.
async fn during_fault(
    cluster: &mut Cluster,
    fixture: &Fixture,
    node: usize,
    restart: bool,
) -> TestResult<Option<Sample>> {
    let mut gets = fixture
        .js
        .client()
        .subscribe(format!(
            "$JS.API.STREAM.MSG.GET.{}",
            fixture.store.config().stream_name()
        ))
        .await?;
    fixture.js.client().flush().await?;
    let fault = async {
        timeout(READ_WAIT, async {
            loop {
                let message = gets.next().await.ok_or("MSG.GET observer closed")?;
                let body: serde_json::Value = serde_json::from_slice(&message.payload)?;
                if body
                    .get("seq")
                    .and_then(serde_json::Value::as_u64)
                    .is_some()
                {
                    break;
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await??;
        if restart {
            cluster.restart(node)?;
        } else {
            cluster.stop(node)?;
        }
        let excluded = format!("receipt-{node}");
        let leader = fixture
            .wait_replicas(
                if restart { 3 } else { 2 },
                (!restart).then_some(excluded.as_str()),
            )
            .await?;
        if !restart {
            assert_ne!(leader, excluded, "failed leader was not replaced");
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let ids = requested()?;
    let (read, fault) = tokio::join!(
        timeout(READ_WAIT, fixture.reader.load_correlations(&ids)),
        fault,
    );
    fault?;
    let phase = if restart {
        "restarting former leader"
    } else {
        "leader failure"
    };
    match read? {
        Ok(read) => Ok(Some(Sample {
            phase,
            read,
            limit: None,
        })),
        Err(error) => {
            assert_eq!(
                error.kind(),
                EventStoreErrorKind::Unavailable,
                "{phase}: {error}"
            );
            eprintln!("bounded transient during {phase}: {error}");
            Ok(None)
        }
    }
}

async fn discovery_owners(
    mut responses: async_nats::Subscriber,
    stream: &str,
) -> TestResult<BTreeSet<String>> {
    responses.unsubscribe().await?;
    let mut owners = BTreeSet::new();
    while let Some(message) = responses.next().await {
        let Ok(info) = serde_json::from_slice::<consumer::Info>(&message.payload) else {
            continue;
        };
        if info.stream_name == stream
            && info.config.durable_name.is_none()
            && info
                .config
                .filter_subjects
                .iter()
                .any(|filter| filter.contains(".transaction.*.receipt."))
        {
            assert_eq!(info.config.num_replicas, 1);
            if let Some(leader) = info.cluster.and_then(|cluster| cluster.leader) {
                owners.insert(leader);
            }
        }
    }
    Ok(owners)
}

async fn lagging_follower(
    cluster: &Cluster,
    fixture: &Fixture,
    workload: &mut Workload,
) -> TestResult<Vec<Sample>> {
    let leader = fixture.wait_replicas(3, None).await?;
    let connected = fixture.js.client().server_info().server_name;
    let node = (0..3)
        .find(|index| {
            let name = format!("receipt-{index}");
            name != leader && name != connected
        })
        .ok_or("no independent follower")?;
    let peer_name = format!("receipt-{node}");
    let before = fixture.journal().await?.cached_info().state.last_sequence;
    let mut fresh = Workload::new()?;
    fresh.next = workload.next;
    for aggregate in &mut fresh.aggregates {
        *aggregate = StreamId::new(
            aggregate.aggregate_type().clone(),
            AggregateId::new(format!("lag-{}", workload.next))?,
        );
    }
    cluster.pause(node, true)?;
    let _resume = ResumeOnDrop { cluster, node };
    // Even a fresh command can load history while materializing its committed
    // outcome. Do not await that replay on a paused node: leader-served INFO
    // independently proves the atomic journal advanced before starting the read.
    let observation = async {
        let journal = timeout(WAIT, async {
            loop {
                let journal = fixture.journal().await?;
                if journal.cached_info().state.last_sequence > before {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(journal);
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await??;
        inspect_lag_window(cluster, fixture, node, &peer_name, journal).await
    };
    let (committed, collected) = tokio::join!(fresh.write(&fixture.store), observation);
    committed?;
    workload.next = fresh.next;
    workload.events.extend(fresh.events);
    workload.aggregates.extend(fresh.aggregates);
    collected
}

async fn inspect_lag_window(
    cluster: &Cluster,
    fixture: &Fixture,
    node: usize,
    peer_name: &str,
    journal: jetstream::stream::Stream,
) -> TestResult<Vec<Sample>> {
    let peer = journal
        .cached_info()
        .cluster
        .as_ref()
        .and_then(|cluster| cluster.replicas.iter().find(|peer| peer.name == peer_name))
        .ok_or("paused peer disappeared from stream placement")?;
    assert!(
        peer.lag.is_some_and(|lag| lag > 0),
        "paused follower did not lag: {peer:?}"
    );
    let cutoff = journal.cached_info().state.last_sequence;
    let mut creates = fixture
        .js
        .client()
        .subscribe(format!(
            "$JS.API.CONSUMER.CREATE.{}.>",
            fixture.store.config().stream_name()
        ))
        .await?;
    // Passive native API evidence only; every request/reply still goes directly
    // between the official client and the broker in this isolated cluster.
    let responses = fixture.js.client().subscribe("_INBOX.>").await?;
    fixture.js.client().flush().await?;
    let resume = async {
        // Resume on CREATE, or after a bounded window if another source's
        // leader is the paused server and capture has not reached CREATE yet.
        tokio::select! {
            request = creates.next() => { request.ok_or("domain discovery observer closed")?; }
            () = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
        cluster.pause(node, false)
    };
    let ids = requested()?;
    let (read, resumed) = tokio::join!(
        timeout(READ_WAIT, fixture.reader.load_correlations(&ids)),
        resume,
    );
    resumed?;
    let owners = timeout(
        READ_WAIT,
        discovery_owners(responses, fixture.store.config().stream_name()),
    )
    .await??;
    eprintln!(
        "lag window: peer={peer_name}, replication lag={:?}, cutoff={cutoff}, native discovery owners={owners:?}",
        peer.lag
    );
    let mut samples = Vec::new();
    match read? {
        Ok(read) => {
            assert_eq!(domain_cutoff(fixture, &read)?, cutoff);
            assert!(
                !owners.is_empty(),
                "successful discovery had no native placement evidence"
            );
            samples.push(Sample {
                phase: "lagging follower resumes during discovery",
                read,
                limit: None,
            });
        }
        Err(error) => {
            assert_eq!(error.kind(), EventStoreErrorKind::Unavailable);
            eprintln!("bounded transient while follower caught up: {error}");
        }
    }
    fixture.wait_replicas(3, None).await?;
    fixture.wait_consumers().await?;
    samples.push(sample(fixture, "settled follower catch-up", false).await?);
    Ok(samples)
}

fn domain_cutoff(fixture: &Fixture, read: &CorrelationRead) -> TestResult<u64> {
    assert_eq!(read.snapshot.sources.len(), 2);
    let integration = read
        .snapshot
        .sources
        .iter()
        .find(|source| source.kind == CorrelationSourceKind::Integration)
        .ok_or("integration source missing")?;
    assert_eq!(
        integration.stream,
        fixture.topology.integration_event_stream().as_str()
    );
    assert!(integration.context.is_none());
    assert_eq!(integration.cutoff_sequence, "0");
    let source = read
        .snapshot
        .sources
        .iter()
        .find(|source| source.kind == CorrelationSourceKind::Domain)
        .ok_or("domain source missing")?;
    assert_eq!(source.stream, fixture.store.config().stream_name());
    assert_eq!(source.context.as_deref(), Some("sales"));
    Ok(source.cutoff_sequence.parse()?)
}

struct KnownEvent {
    correlation: String,
    reference: CorrelationRef,
}

async fn authoritative_events(
    fixture: &Fixture,
    workload: &Workload,
) -> TestResult<BTreeMap<u64, KnownEvent>> {
    let mut known: BTreeMap<_, _> = workload
        .events
        .iter()
        .map(|event| (event.event_id().as_str(), event))
        .collect();
    assert_eq!(
        known.len(),
        workload.events.len(),
        "writer repeated an event identity"
    );
    let journal = fixture.journal().await?;
    let mut result = BTreeMap::new();
    let mut receipts = 0_u32;
    // Independent from receipt decoding/discovery: scan the retained raw journal
    // once after all writes, match the commit results by Nats-Msg-Id, and retain
    // original positions/timestamps. Earlier reads are checked at THEIR cutoff.
    for sequence in 1..=journal.cached_info().state.last_sequence {
        let raw = journal.get_raw_message(sequence).await?;
        if raw.subject.as_str().contains(".aggregate.") {
            let id = raw
                .headers
                .get("Nats-Msg-Id")
                .ok_or("event has no identity")?;
            let event = known
                .remove(id.as_str())
                .ok_or("unknown or duplicate stored event")?;
            assert_eq!(
                raw.subject.as_str(),
                fixture.store.config().event_subject(
                    event.stream_id().aggregate_type().as_str(),
                    event.stream_id().aggregate_id().as_str(),
                )
            );
            let milliseconds = u64::try_from(raw.time.unix_timestamp_nanos() / 1_000_000)?;
            result.insert(
                sequence,
                KnownEvent {
                    correlation: event
                        .correlation_id()
                        .ok_or("writer omitted correlation")?
                        .as_str()
                        .to_owned(),
                    reference: CorrelationRef::Domain(DomainEventRef::from_recorded(
                        "sales",
                        event,
                        MessageTimestamp::from_unix_milliseconds(milliseconds)?,
                    )),
                },
            );
        } else if raw.subject.as_str().contains(".receipt.") {
            receipts = receipts.checked_add(1).ok_or("receipt count overflow")?;
        } else {
            assert!(
                raw.subject.as_str().contains(".guard."),
                "unexpected journal record: {}",
                raw.subject
            );
        }
    }
    assert!(
        known.is_empty(),
        "committed events missing from authoritative journal"
    );
    assert_eq!(
        receipts, workload.next,
        "one permanent receipt per decision"
    );
    Ok(result)
}

fn assert_complete(
    fixture: &Fixture,
    sample: &Sample,
    known: &BTreeMap<u64, KnownEvent>,
) -> TestResult {
    let cutoff = domain_cutoff(fixture, &sample.read)?;
    let mut expected: BTreeMap<_, Vec<_>> = REQUESTED
        .into_iter()
        .map(|id| (id.to_owned(), Vec::new()))
        .collect();
    // With one nonempty source and a generous byte budget, reference limiting
    // must return exactly the first N selected event positions, not any subset.
    let matching: Vec<_> = known
        .range(..=cutoff)
        .filter(|(_, event)| REQUESTED.contains(&event.correlation.as_str()))
        .collect();
    let truncated = sample.limit.is_some_and(|limit| matching.len() > limit);
    for (_, event) in matching.iter().take(sample.limit.unwrap_or(usize::MAX)) {
        expected
            .get_mut(&event.correlation)
            .ok_or("unrequested oracle event")?
            .push(event.reference.clone());
    }
    assert_eq!(sample.read.chains.len(), REQUESTED.len());
    for chain in &sample.read.chains {
        let mut references = expected
            .remove(&chain.correlation_id)
            .ok_or("unexpected/duplicate chain")?;
        references.sort_by(|left, right| {
            left.occurred_at()
                .cmp(&right.occurred_at())
                .then_with(|| left.identity().cmp(&right.identity()))
        });
        assert_eq!(
            chain.truncated, truncated,
            "{} cutoff {cutoff}: {}",
            sample.phase, chain.correlation_id
        );
        assert_eq!(
            chain.refs, references,
            "{} cutoff {cutoff}: {} omitted, duplicated, invented or moved events",
            sample.phase, chain.correlation_id
        );
    }
    assert!(expected.is_empty());
    Ok(())
}

async fn qualify(cluster: &mut Cluster) -> TestResult {
    let fixture = Fixture::new(cluster).await?;
    let mut workload = Workload::new()?;
    for _ in 0..24 {
        workload.write(&fixture.store).await?;
    }
    let journal = fixture.journal().await?;
    let mut durable: consumer::PullConsumer = journal
        .create_consumer(consumer::pull::Config {
            durable_name: Some(DURABLE.to_owned()),
            filter_subject: format!("{}.aggregate.*.*", fixture.store.config().subject_prefix()),
            ..Default::default()
        })
        .await?;
    let mut delivery = durable.fetch().max_messages(1).messages().await?;
    let message = timeout(READ_WAIT, delivery.next())
        .await?
        .ok_or("durable has no event")??;
    message.double_ack().await?;
    drop(delivery);
    let progress = durable.info().await?.clone();
    assert_eq!(progress.ack_floor.consumer_sequence, 1);

    let mut samples = vec![
        sample(&fixture, "baseline", false).await?,
        sample(&fixture, "baseline budget", true).await?,
    ];
    samples.extend(while_writing(&fixture, &mut workload, "live R3").await?);
    fixture.assert_consumers().await?;
    samples.extend(lagging_follower(cluster, &fixture, &mut workload).await?);

    let leader = fixture.wait_replicas(3, None).await?;
    let node: usize = leader
        .strip_prefix("receipt-")
        .ok_or("unknown leader")?
        .parse()?;
    if let Some(sample) = during_fault(cluster, &fixture, node, false).await? {
        samples.push(sample);
    }
    if let Some(sample) = during_fault(cluster, &fixture, node, true).await? {
        samples.push(sample);
    }
    // An ephemeral assigned to the killed node cannot execute its inactivity
    // timer until restart. Verify deletion after every process can run again.
    fixture.wait_consumers().await?;
    samples.extend(while_writing(&fixture, &mut workload, "recovered R3").await?);
    fixture.wait_replicas(3, None).await?;
    let final_sample = sample(&fixture, "settled restart", false).await?;
    assert_eq!(
        domain_cutoff(&fixture, &final_sample.read)?,
        fixture.journal().await?.cached_info().state.last_sequence,
        "settled read must reach every acknowledged transaction"
    );
    samples.push(final_sample);
    let known = authoritative_events(&fixture, &workload).await?;
    for sample in &samples {
        assert_complete(&fixture, sample, &known)?;
    }
    // Full aggregate reconstruction still uses the stable aggregate subjects.
    for aggregate in &workload.aggregates {
        let expected: Vec<_> = workload
            .events
            .iter()
            .filter(|event| event.stream_id() == aggregate)
            .cloned()
            .collect();
        assert_eq!(fixture.store.load(aggregate).await?, expected);
    }
    fixture.assert_consumers().await?;
    let final_progress = durable.info().await?;
    assert_eq!(final_progress.created, progress.created);
    // Leadership can change the broker's last_active timestamp. Application
    // progress is the exact delivered/acknowledged consumer and stream cursors.
    for (current, initial) in [
        (final_progress.delivered, progress.delivered),
        (final_progress.ack_floor, progress.ack_floor),
    ] {
        assert_eq!(current.consumer_sequence, initial.consumer_sequence);
        assert_eq!(current.stream_sequence, initial.stream_sequence);
    }
    assert_eq!(final_progress.num_ack_pending, progress.num_ack_pending);
    assert_eq!(final_progress.num_redelivered, progress.num_redelivered);
    eprintln!(
        "R3/file: {} decisions, {} events, {} cutoff-checked reads; failover/restart preserved journal and durable progress",
        workload.next,
        known.len(),
        samples.len()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires native nats-server; owns a three-broker R3/file cluster"]
async fn receipt_cutoffs_remain_complete_during_writes_and_leader_restart() -> TestResult {
    let mut cluster = Cluster::start("receipt")?;
    timeout(Duration::from_secs(240), qualify(&mut cluster)).await??;
    cluster.succeeded = true;
    Ok(())
}
