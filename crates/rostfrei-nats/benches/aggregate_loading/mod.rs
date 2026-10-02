pub mod config;
pub mod model;
pub mod report;
pub mod seed;

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rostfrei_core::{
    AggregateInstance, CommandExecutor, CommandOutcome, CommandReceipt, Event, EventHistory,
    EventStoreError, InMemoryEventStore, RecordedEvent, StreamId,
};
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    NatsConnection, NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, ServerVersion,
    connect, provision_event_store,
};

use config::HistoryKind;
pub use config::Options;
use model::{InventoryAggregate, StockReceived};
use report::{CaseReport, Phase, READ_PHASES, Report, Sample, nanos};

pub type BenchResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct MeasuredHistory {
    store: NatsEventStore,
    last_load_ns: AtomicU64,
    audit: bool,
}

#[async_trait]
impl EventHistory for MeasuredHistory {
    async fn load(&self, stream: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        let started = Instant::now();
        let result = if self.audit {
            self.store.audit_history(stream).await
        } else {
            self.store.load(stream).await
        };
        self.last_load_ns.store(
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        result
    }
}

struct ReadCase<'a> {
    executor: CommandExecutor<MeasuredHistory>,
    connection: &'a NatsConnection,
    stream: StreamId,
    expected_events: u64,
    note_bytes: u32,
    decoded: Vec<StockReceived>,
}

impl ReadCase<'_> {
    async fn measure(&self, phase: Phase, iteration: u32) -> BenchResult<Sample> {
        // Repeated apply-only samples own fresh typed inputs before timing starts.
        let apply_input = (phase == Phase::ApplyOnly).then(|| self.decoded.clone());
        self.connection.client().flush().await?;
        let before = outgoing(self.connection);
        let started = Instant::now();
        let (elapsed_ns, history_load_ns) = match phase {
            Phase::HistoryLoad => {
                let events = self.executor.store().load(&self.stream).await?;
                let elapsed = nanos(started.elapsed())?;
                if events.len() != usize::try_from(self.expected_events)? {
                    return Err("history length changed during read benchmark".into());
                }
                std::hint::black_box(&events);
                (elapsed, None)
            }
            Phase::Rehydrate => {
                let aggregate = self
                    .executor
                    .rehydrate::<InventoryAggregate>(&self.stream)
                    .await?;
                let elapsed = nanos(started.elapsed())?;
                let history = self.executor.store().last_load_ns.load(Ordering::Relaxed);
                model::verify(aggregate.state(), self.expected_events, self.note_bytes)?;
                std::hint::black_box(&aggregate);
                (elapsed, Some(history))
            }
            Phase::ApplyOnly => {
                let aggregate = AggregateInstance::<InventoryAggregate>::rehydrate(
                    self.stream.clone(),
                    apply_input.ok_or("apply inputs are missing")?,
                );
                let elapsed = nanos(started.elapsed())?;
                model::verify(aggregate.state(), self.expected_events, self.note_bytes)?;
                std::hint::black_box(&aggregate);
                (elapsed, None)
            }
            Phase::ExecuteCommand => return Err("write phase cannot run as a read sample".into()),
        };
        self.connection.client().flush().await?;
        Ok(Sample {
            phase,
            iteration,
            events_before: self.expected_events,
            elapsed_ns,
            nats_out_messages: outgoing(self.connection)
                .checked_sub(before)
                .ok_or("request counter regressed")?,
            history_load_ns,
            after_history_ns: history_load_ns
                .map(|history| {
                    elapsed_ns
                        .checked_sub(history)
                        .ok_or("history duration exceeds full rehydration")
                })
                .transpose()?,
        })
    }
}

fn outgoing(connection: &NatsConnection) -> u64 {
    connection
        .client()
        .statistics()
        .out_messages
        .load(Ordering::Relaxed)
}

async fn measure_case(
    store: &NatsEventStore,
    connection: &NatsConnection,
    options: &Options,
    kind: HistoryKind,
    count: u32,
) -> BenchResult<CaseReport> {
    let id = format!("{}-{count}-{}", kind.name(), options.note_bytes);
    eprintln!("preparing {id}");
    let setup = Instant::now();
    seed::seed(store.clone(), &id, kind, count, options.note_bytes).await?;
    let stream = model::stream(&id)?;
    let recorded = store.load(&stream).await?;
    let seeded_domain_payload_bytes = recorded
        .iter()
        .try_fold(0_usize, |total, event| {
            total.checked_add(event.payload().len())
        })
        .ok_or("fixture bytes overflow")?;
    let decoded = recorded
        .iter()
        .map(StockReceived::decode_json)
        .collect::<Result<Vec<_>, _>>()?;
    drop(recorded);
    let reads = ReadCase {
        connection,
        stream,
        expected_events: u64::from(count),
        note_bytes: options.note_bytes,
        executor: CommandExecutor::new(MeasuredHistory {
            store: store.clone(),
            last_load_ns: AtomicU64::new(0),
            audit: options.audit_reads,
        }),
        decoded,
    };
    let setup_ns = nanos(setup.elapsed())?;
    for iteration in 0..options.warmups {
        for phase in READ_PHASES {
            reads.measure(phase, iteration).await?;
        }
    }
    let mut samples = Vec::new();
    for iteration in 0..options.samples {
        let offset = usize::try_from(iteration)?.rem_euclid(READ_PHASES.len());
        for &phase in READ_PHASES
            .iter()
            .skip(offset)
            .chain(READ_PHASES.iter().take(offset))
        {
            samples.push(reads.measure(phase, iteration).await?);
        }
    }
    // Warm writes use another aggregate so read samples and the first timed
    // command retain the advertised base history size.
    let warmup_id = format!("{id}-warmup");
    seed::seed(
        store.clone(),
        &warmup_id,
        HistoryKind::Commands,
        options.warmups,
        options.note_bytes,
    )
    .await?;
    for iteration in 0..options.samples {
        samples.push(measure_command(store, connection, options, &id, count, iteration).await?);
    }
    let final_count = u64::from(count)
        .checked_add(u64::from(options.samples))
        .ok_or("final count overflow")?;
    let final_state = reads
        .executor
        .rehydrate::<InventoryAggregate>(&reads.stream)
        .await?;
    model::verify(final_state.state(), final_count, options.note_bytes)?;
    let summaries = report::summarize(&samples)?;
    for summary in &summaries {
        eprintln!(
            "{id} {:?}: median {:.3?}, range {:.3?}..{:.3?}, requests {}..{}",
            summary.phase,
            Duration::from_nanos(summary.median_ns),
            Duration::from_nanos(summary.min_ns),
            Duration::from_nanos(summary.max_ns),
            summary.min_out_messages,
            summary.max_out_messages
        );
        if let Some(after) = summary.median_after_history_ns {
            eprintln!(
                "  median runtime validation + typed decode + apply after history: {:.3?}",
                Duration::from_nanos(after)
            );
        }
    }
    Ok(CaseReport {
        history: kind,
        seeded_events: count,
        seeded_domain_payload_bytes,
        setup_ns,
        samples,
        summaries,
    })
}

async fn measure_command(
    store: &NatsEventStore,
    connection: &NatsConnection,
    options: &Options,
    id: &str,
    count: u32,
    iteration: u32,
) -> BenchResult<Sample> {
    let events_before = u64::from(count)
        .checked_add(u64::from(iteration))
        .ok_or("sample count overflow")?;
    let command = seed::command(id, events_before, options.note_bytes)?;
    let metadata = model::metadata(&format!("{id}:measured:{iteration}"), &command)?;
    let executor = CommandExecutor::new(store.clone());
    connection.client().flush().await?;
    let before = outgoing(connection);
    let started = Instant::now();
    let outcome = executor
        .execute(&model::ReceiveStockHandler, metadata, &command)
        .await?;
    let elapsed_ns = nanos(started.elapsed())?;
    seed::appended(outcome)?;
    connection.client().flush().await?;
    Ok(Sample {
        phase: Phase::ExecuteCommand,
        iteration,
        events_before,
        elapsed_ns,
        nats_out_messages: outgoing(connection)
            .checked_sub(before)
            .ok_or("request counter regressed")?,
        history_load_ns: None,
        after_history_ns: None,
    })
}

pub async fn run(options: Options) -> BenchResult {
    if options.test || (cfg!(test) && cfg!(debug_assertions)) {
        eprintln!("Benchmark test mode: verifying in-memory fixture contracts");
        return smoke().await;
    }
    if cfg!(debug_assertions) {
        return Err("run this suite with cargo bench (optimized profile)".into());
    }
    let url = std::env::var("ROSTFREI_NATS_URL").map_err(
        |_| "ROSTFREI_NATS_URL is required; use scripts/test_nats.py with this benchmark",
    )?;
    if url.trim().is_empty() {
        return Err("ROSTFREI_NATS_URL must not be empty".into());
    }
    let connection = connect(
        &NatsConnectionConfig::new("aggregate-benchmark", url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;
    let started = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let unique = started.as_nanos();
    let application = ApplicationName::new(format!("bench-{}-{unique}", std::process::id()))?;
    let context = application.test_bounded_context(model::CONTEXT)?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(options.stream_bytes, 512 * 1024)?;
    let measured: BenchResult<Vec<CaseReport>> = async {
        provision_event_store(connection.jetstream(), &config).await?;
        let store = NatsEventStore::connect(connection.jetstream().clone(), config.clone()).await?;
        let mut cases = Vec::new();
        for &kind in &options.history {
            for &count in &options.events {
                cases.push(
                    tokio::time::timeout(
                        Duration::from_secs(options.case_timeout_seconds),
                        measure_case(&store, &connection, &options, kind, count),
                    )
                    .await
                    .map_err(|_| "benchmark case timed out (including fixture creation)")??,
                );
            }
        }
        Ok(cases)
    }
    .await;
    let cleanup = connection
        .delete_stream_if_exists(config.stream_name())
        .await;
    let server_version = connection.client().server_info().version;
    let drain = connection.drain().await;
    let cases = measured?;
    cleanup?;
    drain?;
    let rustc = command_output("rustc", &["--version"]).unwrap_or_else(|| "unavailable".to_owned());
    let report = Report {
        schema_version: 1,
        started_at_unix_ms: u64::try_from(started.as_millis())?,
        git_revision: command_output("git", &["rev-parse", "HEAD"]),
        git_dirty: command_output("git", &["status", "--porcelain"])
            .map(|status| !status.is_empty()),
        framework_version: env!("CARGO_PKG_VERSION"),
        rustc,
        nats_server_version: server_version,
        operating_system: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        available_parallelism: std::thread::available_parallelism()
            .ok()
            .map(std::num::NonZeroUsize::get),
        workload: "inventory: 64 SKUs, JSON stock-received events, BTreeMap projection",
        options,
        cases,
    };
    report::write(&report, &report.options.output)?;
    eprintln!("Benchmark report: {}", report.options.output.display());
    Ok(())
}

fn command_output(program: &str, arguments: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub async fn smoke() -> BenchResult {
    for kind in [HistoryKind::Direct, HistoryKind::Commands] {
        for count in [0, 3, 131] {
            let store = InMemoryEventStore::new();
            seed::seed(store.clone(), "smoke", kind, count, 32).await?;
            let executor = CommandExecutor::new(store);
            let stream = model::stream("smoke")?;
            let aggregate = executor.rehydrate::<InventoryAggregate>(&stream).await?;
            model::verify(aggregate.state(), u64::from(count), 32)?;
            let command = seed::command("smoke", u64::from(count), 32)?;
            let metadata = model::metadata("smoke-write", &command)?;
            seed::appended(
                executor
                    .execute(&model::ReceiveStockHandler, metadata.clone(), &command)
                    .await?,
            )?;
            if !matches!(
                executor
                    .execute(&model::ReceiveStockHandler, metadata, &command)
                    .await?,
                CommandOutcome::Accepted(CommandReceipt::ExactReplay(_))
            ) {
                return Err("benchmark fixture did not preserve exact command replay".into());
            }
            let aggregate = executor.rehydrate::<InventoryAggregate>(&stream).await?;
            model::verify(aggregate.state(), u64::from(count).saturating_add(1), 32)?;
        }
    }
    Ok(())
}
