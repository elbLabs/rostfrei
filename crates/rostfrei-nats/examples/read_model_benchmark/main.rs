//! Release-mode, real-NATS comparison of equivalent typed query handlers.
//! See docs/read-model-benchmark.md for the measured boundary and commands.

mod metrics;
mod model;

use std::{
    num::NonZeroU32,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use clap::Parser;
use metrics::{Samples, Summary, Traffic};
use model::{AccessView, BillingEvent, OrganizationEvent, Payload, Snapshot};
use rostfrei::{
    ApplicationName, JsonReadModelCodec, QueryErrorPayload, QueryHandler, QueryHandlerRequest,
    ReadModelCodec, ReadModelKey, ReadModelStore, TrafficScope,
};
use rostfrei_core::{
    AggregateId, AggregateType, ContentFingerprint, EventBatch, EventStore, ExpectedVersion,
    NewEvent, OperationId, RecordedEvent, StreamId, StreamVersion, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::{
    CallerMetadata, CorrelationId, EnvelopeContext, MessageId, MessageTimestamp, SchemaVersion,
};
use rostfrei_nats::{
    NatsConnection, NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig,
    NatsReadModelConfig, NatsReadModelStore, connect, provision_event_store, provision_read_model,
};
use serde::{Deserialize, Serialize};

type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type SnapshotStore = NatsReadModelStore<Snapshot, JsonReadModelCodec<Snapshot>>;

#[derive(Parser, Serialize)]
#[command(about = "Compare one KV snapshot query with replay of two linked aggregates")]
struct Options {
    #[arg(long, value_delimiter = ',', default_value = "10,50,100")]
    events_per_aggregate: Vec<u32>,
    /// Samples per path per round. Every sample must return the same JSON result.
    #[arg(long, default_value_t = 100)]
    samples: u32,
    #[arg(long, default_value_t = 3)]
    rounds: u32,
    #[arg(long, default_value_t = 10)]
    warmup: u32,
    /// Events grouped into each source commit (one represents one-event commands).
    #[arg(long, default_value_t = 1)]
    events_per_commit: u32,
    /// Padding in each domain-event payload, beyond event fields/JSON framing.
    #[arg(long, default_value_t = 256)]
    extra_event_bytes: usize,
}

impl Options {
    fn validate(&self) -> BenchResult {
        if self.events_per_aggregate.is_empty()
            || self
                .events_per_aggregate
                .iter()
                .any(|n| !(2..=1000).contains(n))
            || !(1..=10_000).contains(&self.samples)
            || !(1..=20).contains(&self.rounds)
            || self.warmup > 1000
            || !(1..=99).contains(&self.events_per_commit)
            || self.extra_event_bytes > 64 * 1024
        {
            return Err("invalid benchmark bounds; use --help (events 2..1000, samples 1..10000, rounds 1..20, warmup <=1000, commit 1..99, padding <=65536)".into());
        }
        let unique: std::collections::BTreeSet<_> = self.events_per_aggregate.iter().collect();
        if unique.len() != self.events_per_aggregate.len() {
            return Err("duplicate event-count cases".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Path {
    Kv,
    Sequential,
    Parallel,
    Cached,
}

impl Path {
    const ALL: [Self; 4] = [Self::Kv, Self::Sequential, Self::Parallel, Self::Cached];
    const fn name(self) -> &'static str {
        match self {
            Self::Kv => "kv_snapshot",
            Self::Sequential => "replay_sequential",
            Self::Parallel => "replay_parallel",
            Self::Cached => "replay_loaded_history",
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct AccessQuery {
    organization_id: String,
}

struct Fixture {
    history: NatsEventStore,
    snapshots: SnapshotStore,
    organization: StreamId,
    billing: StreamId,
    organization_events: Vec<RecordedEvent>,
    billing_events: Vec<RecordedEvent>,
}

impl Fixture {
    async fn query(&self, path: Path, request: &AccessQuery) -> BenchResult<AccessView> {
        if request.organization_id != self.organization.aggregate_id().as_str() {
            return Err("query organization mismatch".into());
        }
        let snapshot = match path {
            Path::Kv => {
                self.snapshots
                    .read(&ReadModelKey::new(&request.organization_id)?)
                    .await?
                    .ok_or("snapshot missing")?
                    .value
            }
            Path::Sequential => {
                let records = self.history.load(&self.organization).await?;
                let state = model::replay_organization(&records)?;
                let billing = StreamId::new(
                    AggregateType::new("billing-account")?,
                    AggregateId::new(&state.billing_account_id)?,
                );
                let records = self.history.load(&billing).await?;
                model::join(&self.organization, state, &model::replay_billing(&records)?)?
            }
            Path::Parallel => {
                let (organization, billing) = tokio::try_join!(
                    self.history.load(&self.organization),
                    self.history.load(&self.billing)
                )?;
                model::replay(&self.organization, &organization, &billing)?
            }
            Path::Cached => model::replay(
                &self.organization,
                &self.organization_events,
                &self.billing_events,
            )?,
        };
        if snapshot.view.billing_account_id != self.billing.aggregate_id().as_str() {
            return Err("aggregate relation mismatch".into());
        }
        Ok(snapshot.view)
    }
}

struct Handler<'a> {
    fixture: &'a Fixture,
    path: Path,
}

#[async_trait]
impl QueryHandler<AccessQuery, AccessView> for Handler<'_> {
    async fn handle(
        &self,
        request: QueryHandlerRequest<AccessQuery>,
    ) -> Result<AccessView, QueryErrorPayload> {
        self.fixture
            .query(self.path, request.payload())
            .await
            .map_err(|error| {
                eprintln!("benchmark query failed: {error}");
                QueryErrorPayload::internal_error()
            })
    }
}

#[derive(Serialize)]
struct CaseReport {
    events_per_aggregate: u32,
    total_events: u32,
    source_payload_bytes: usize,
    encoded_snapshot_bytes: usize,
    encoded_response_bytes: usize,
    results: Vec<Summary>,
}

#[derive(Serialize)]
struct Report {
    release_build: bool,
    nats_server_version: String,
    architecture: &'static str,
    os: &'static str,
    runtime_workers: usize,
    options: Options,
    cases: Vec<CaseReport>,
}

#[tokio::main(worker_threads = 2)]
async fn main() -> BenchResult {
    let options = Options::parse();
    options.validate()?;
    if cfg!(debug_assertions) {
        eprintln!("Debug build: use --release for publishable timings.");
    }
    let connection = connect(&NatsConnectionConfig::new(
        "read-model-benchmark",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let application = ApplicationName::new(format!("kv-bench-{nonce}"))?;
    let context = application.bounded_context_in_scope(TrafficScope::Test, "access")?;
    let events = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(256 * 1024 * 1024, 512 * 1024)?;
    let models = options
        .events_per_aggregate
        .iter()
        .map(|size| NatsReadModelConfig::new(&context, format!("access-{size}")))
        .collect::<Result<Vec<_>, _>>()?;
    let result = run(&connection, &events, &models, &options).await;
    let cleanup = cleanup(&connection, &events, &models).await;
    let cases = result?;
    cleanup?;
    let report = Report {
        release_build: !cfg!(debug_assertions),
        nats_server_version: connection.client().server_info().version,
        architecture: std::env::consts::ARCH,
        os: std::env::consts::OS,
        runtime_workers: 2,
        options,
        cases,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn cleanup(
    connection: &NatsConnection,
    events: &NatsEventStoreConfig,
    models: &[NatsReadModelConfig],
) -> BenchResult {
    for model in models {
        connection
            .delete_stream_if_exists(&model.stream_name())
            .await?;
    }
    connection
        .delete_stream_if_exists(events.stream_name())
        .await?;
    Ok(())
}

async fn run(
    connection: &NatsConnection,
    events: &NatsEventStoreConfig,
    models: &[NatsReadModelConfig],
    options: &Options,
) -> BenchResult<Vec<CaseReport>> {
    provision_event_store(connection.jetstream(), events).await?;
    let history = NatsEventStore::connect(connection.jetstream().clone(), events.clone()).await?;
    let mut reports = Vec::new();
    for (size, config) in options.events_per_aggregate.iter().zip(models) {
        eprintln!("Preparing two aggregates with {size} events each...");
        let fixture = prepare(connection, &history, config, *size, options).await?;
        reports.push(measure(connection.client(), &fixture, *size, options).await?);
    }
    Ok(reports)
}

async fn prepare(
    connection: &NatsConnection,
    history: &NatsEventStore,
    config: &NatsReadModelConfig,
    size: u32,
    options: &Options,
) -> BenchResult<Fixture> {
    let organization = StreamId::new(
        AggregateType::new("organization")?,
        AggregateId::new(format!("org-{size}"))?,
    );
    let billing = StreamId::new(
        AggregateType::new("billing-account")?,
        AggregateId::new(format!("billing-{size}"))?,
    );
    let note = "x".repeat(options.extra_event_bytes);
    let mut organization_payloads = Vec::new();
    let mut billing_payloads = Vec::new();
    for index in 0..size {
        let change = if index == 0 {
            OrganizationEvent::Created {
                billing_account_id: billing.aggregate_id().as_str().to_owned(),
            }
        } else {
            OrganizationEvent::MemberJoined {
                member_id: format!("member-{index}"),
            }
        };
        organization_payloads.push(serde_json::to_vec(&Payload {
            change,
            note: note.clone(),
        })?);
        let change = if index == 0 {
            BillingEvent::Opened {
                organization_id: organization.aggregate_id().as_str().to_owned(),
            }
        } else {
            BillingEvent::SeatsPurchased { seats: 2 }
        };
        billing_payloads.push(serde_json::to_vec(&Payload {
            change,
            note: note.clone(),
        })?);
    }
    seed(
        history,
        &organization,
        "organization-changed",
        &organization_payloads,
        options.events_per_commit,
    )
    .await?;
    seed(
        history,
        &billing,
        "billing-changed",
        &billing_payloads,
        options.events_per_commit,
    )
    .await?;
    let organization_events = history.load(&organization).await?;
    let billing_events = history.load(&billing).await?;
    let snapshot = model::replay(&organization, &organization_events, &billing_events)?;
    let changes = u64::from(size.checked_sub(1).ok_or("empty history")?);
    if snapshot.view.member_count != changes
        || snapshot.view.purchased_seats != changes.checked_mul(2).ok_or("seats overflow")?
        || snapshot.view.available_seats != changes
        || snapshot.organization_version != u64::from(size)
        || snapshot.billing_version != u64::from(size)
    {
        return Err("replayed result disagrees with seeded facts".into());
    }
    provision_read_model(connection.jetstream(), config).await?;
    let snapshots = SnapshotStore::connect(
        connection.jetstream().clone(),
        config.clone(),
        JsonReadModelCodec::new(NonZeroU32::MIN),
    )
    .await?;
    snapshots
        .create(
            &ReadModelKey::new(organization.aggregate_id().as_str())?,
            &snapshot,
        )
        .await?;
    Ok(Fixture {
        history: history.clone(),
        snapshots,
        organization,
        billing,
        organization_events,
        billing_events,
    })
}

async fn seed(
    history: &NatsEventStore,
    stream: &StreamId,
    event_type: &str,
    payloads: &[Vec<u8>],
    events_per_commit: u32,
) -> BenchResult {
    let mut version = 0_u64;
    for (index, chunk) in payloads
        .chunks(usize::try_from(events_per_commit)?)
        .enumerate()
    {
        let operation = OperationId::new(format!(
            "{}-{}-commit-{index}",
            stream.aggregate_type().as_str(),
            stream.aggregate_id().as_str()
        ))?;
        let commit = derive_commit_id(stream, &operation);
        let events = chunk
            .iter()
            .enumerate()
            .map(|(ordinal, payload)| {
                Ok(NewEvent::new(
                    derive_event_id(&commit, u32::try_from(ordinal)?),
                    event_type,
                    1,
                    payload.clone(),
                )?)
            })
            .collect::<BenchResult<Vec<_>>>()?;
        let fingerprint = ContentFingerprint::digest(serde_json::to_vec(chunk)?);
        let expected = if version == 0 {
            ExpectedVersion::NoStream
        } else {
            ExpectedVersion::Exact(StreamVersion::new(version))
        };
        history
            .append(
                stream,
                expected,
                EventBatch::new(commit, operation, fingerprint, events)?,
            )
            .await?;
        version = version
            .checked_add(u64::try_from(chunk.len())?)
            .ok_or("source version overflow")?;
    }
    Ok(())
}

fn request(fixture: &Fixture) -> BenchResult<QueryHandlerRequest<AccessQuery>> {
    Ok(QueryHandlerRequest::new(
        EnvelopeContext::new(
            MessageId::new("benchmark-query")?,
            SchemaVersion::new(1)?,
            CorrelationId::new("benchmark")?,
            None,
        ),
        MessageTimestamp::from_unix_milliseconds(1)?,
        CallerMetadata::default(),
        None,
        AccessQuery {
            organization_id: fixture.organization.aggregate_id().as_str().to_owned(),
        },
    )?)
}

async fn execute(
    handler: &Handler<'_>,
    request: QueryHandlerRequest<AccessQuery>,
) -> BenchResult<Vec<u8>> {
    let response = handler
        .handle(request)
        .await
        .map_err(|error| error.message().to_owned())?;
    Ok(serde_json::to_vec(&response)?)
}

async fn measure(
    client: &async_nats::Client,
    fixture: &Fixture,
    size: u32,
    options: &Options,
) -> BenchResult<CaseReport> {
    let request = request(fixture)?;
    let snapshot = model::replay(
        &fixture.organization,
        &fixture.organization_events,
        &fixture.billing_events,
    )?;
    let expected = serde_json::to_vec(&snapshot.view)?;
    let mut paths = Path::ALL.map(|path| (path, Samples::default()));
    for _ in 0..options.warmup {
        for path in Path::ALL {
            if execute(&Handler { fixture, path }, request.clone()).await? != expected {
                return Err("warmup query mismatch".into());
            }
        }
    }
    client.flush().await?;
    for round in 0..options.rounds {
        eprintln!(
            "Measuring {size} events/aggregate, round {}/{}, {} samples/path...",
            round.saturating_add(1),
            options.rounds,
            options.samples
        );
        for _ in 0..options.samples {
            for (path, samples) in &mut paths {
                let handler = Handler {
                    fixture,
                    path: *path,
                };
                let request = request.clone();
                let before = Traffic::capture(client);
                let start = Instant::now();
                let response = execute(&handler, request).await?;
                let elapsed = start.elapsed();
                let after = Traffic::capture(client);
                if std::hint::black_box(&response) != &expected {
                    return Err("measured query mismatch".into());
                }
                samples.record(elapsed, before, after)?;
            }
            paths.rotate_left(1);
        }
    }
    let mut results = paths
        .into_iter()
        .map(|(path, samples)| samples.summarize(path.name()))
        .collect::<BenchResult<Vec<_>>>()?;
    results.sort_by_key(|result| result.path);
    let source_payload_bytes = fixture
        .organization_events
        .iter()
        .chain(&fixture.billing_events)
        .try_fold(0_usize, |total, event| {
            total
                .checked_add(event.payload().len())
                .ok_or("source payload length overflow")
        })?;
    Ok(CaseReport {
        events_per_aggregate: size,
        total_events: size.checked_mul(2).ok_or("event count overflow")?,
        source_payload_bytes,
        encoded_snapshot_bytes: JsonReadModelCodec::new(NonZeroU32::MIN)
            .encode(&snapshot)?
            .len(),
        encoded_response_bytes: expected.len(),
        results,
    })
}
