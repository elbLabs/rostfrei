//! Process-isolated large-history measurements; see `scripts/benchmark_snapshots.py`.
//! Only fixture import bypasses admission. All timed reads, captures and commands
//! use production APIs and validate the complete inventory result.
#![allow(
    clippy::too_many_lines,
    reason = "standalone benchmark operation dispatch"
)]
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Parser, ValueEnum};
use futures_util::StreamExt as _;
use rostfrei_benchmarks::{
    BenchResult,
    aggregate_loading::{model, seed},
};
use rostfrei_core::{
    AggregateCodecs, CommandExecutor, ContentFingerprint, Event, JsonSnapshotCodec, OperationId,
    RebuildDiagnostics, SnapshotCodec, SnapshotPolicy, StreamVersion, derive_commit_id,
    derive_event_id,
};
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    NatsConnection, NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig,
    NatsReadModelConfig, NatsSnapshotStore, connect, provision_event_store, provision_read_model,
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Action {
    Init,
    Seed,
    Audit,
    Capture,
    Read,
    Command,
    Storage,
    Head,
    Cleanup,
}

#[derive(Parser)]
struct Options {
    #[arg(long)]
    action: Action,
    #[arg(long)]
    application: String,
    #[arg(long, default_value = "baseline")]
    scenario: String,
    #[arg(long, default_value_t = 0)]
    start: u64,
    #[arg(long, default_value_t = 0)]
    events: u64,
    #[arg(long, default_value_t = 3)]
    samples: u32,
    #[arg(long, default_value_t = 1)]
    warmups: u32,
}

const ID: &str = "single";
const NOTE_BYTES: u32 = 128;
const GIB: i64 = 1024 * 1024 * 1024;

fn main() -> BenchResult {
    if cfg!(debug_assertions) {
        return Err("use a release build".into());
    }
    let options = Options::parse();
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async { tokio::spawn(run(options)).await? })
}

async fn run(options: Options) -> BenchResult {
    let connection = connect(&NatsConnectionConfig::new(
        "snapshot-scaling",
        std::env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let context =
        ApplicationName::new(&options.application)?.test_bounded_context(model::CONTEXT)?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_split_outcome_epoch("isolated-scaling-benchmark")?
        .with_storage_limits(64 * GIB, 512 * 1024)?;
    let snapshot_config = |scenario: &str| -> BenchResult<NatsReadModelConfig> {
        Ok(
            NatsReadModelConfig::new(&context, format!("snapshots-{scenario}"))?
                .with_storage_limits(256 * 1024 * 1024, 2 * 1024 * 1024)?,
        )
    };
    if matches!(options.action, Action::Init) {
        provision_event_store(connection.jetstream(), &config).await?;
        for scenario in ["a", "b", "c"] {
            provision_read_model(connection.jetstream(), &snapshot_config(scenario)?).await?;
        }
        println!(
            "{}",
            json!({"action":"init", "server":connection.client().server_info().version, "journal":config.stream_name()})
        );
    } else if matches!(options.action, Action::Cleanup) {
        connection
            .delete_stream_if_exists(config.stream_name())
            .await?;
        connection
            .delete_stream_if_exists(config.outcome_state_stream_name())
            .await?;
        for scenario in ["a", "b", "c"] {
            connection
                .delete_stream_if_exists(&snapshot_config(scenario)?.stream_name())
                .await?;
        }
        println!("{}", json!({"action":"cleanup", "streams_removed":true}));
    } else if matches!(options.action, Action::Storage) {
        let mut values = Vec::new();
        for (kind, name) in [
            ("journal", config.stream_name().to_owned()),
            ("outcomes", config.outcome_state_stream_name().to_owned()),
            ("a", snapshot_config("a")?.stream_name()),
            ("b", snapshot_config("b")?.stream_name()),
            ("c", snapshot_config("c")?.stream_name()),
        ] {
            let stream = connection.jetstream().get_stream(name.clone()).await?;
            let info = stream.cached_info();
            values.push(json!({"kind":kind,"stream":name,"bytes":info.state.bytes,"messages":info.state.messages}));
        }
        println!("{}", json!({"action":"storage","streams":values}));
    } else if matches!(options.action, Action::Head) {
        let stream = model::stream(ID)?;
        let journal = connection
            .jetstream()
            .get_stream(config.stream_name())
            .await?;
        let message = journal
            .get_last_raw_message_by_subject(
                &config.aggregate_subject(stream.aggregate_type().as_str(), ID),
            )
            .await?;
        let envelope: Value = serde_json::from_slice(&message.payload)?;
        let head = envelope
            .get("event")
            .and_then(|value| value.get("streamVersion"))
            .and_then(Value::as_u64)
            .ok_or("missing fixture head")?;
        println!("{}", json!({"action":"head","head_after":head}));
    } else if matches!(options.action, Action::Seed) {
        import_fixture(&connection, &config, options.start, options.events).await?;
    } else {
        let store = NatsEventStore::connect(connection.jetstream().clone(), config)
            .await?
            .with_snapshotting(options.scenario != "baseline");
        let diagnostics = Arc::new(RebuildDiagnostics::new(4));
        let mut codecs = AggregateCodecs::default().with_rebuild_diagnostics(diagnostics.clone());
        if options.scenario != "baseline" {
            codecs = codecs.with_snapshot_store(Arc::new(
                NatsSnapshotStore::connect(
                    connection.jetstream().clone(),
                    snapshot_config(&options.scenario)?,
                )
                .await?,
            ));
            codecs.register_json_snapshot::<model::InventoryAggregate>(
                "inventory-v1",
                1,
                SnapshotPolicy::Manual,
            )?;
        }
        let executor = CommandExecutor::with_codecs(store, Arc::new(codecs));
        let stream = model::stream(ID)?;
        match options.action {
            Action::Audit => {
                let events = executor.store().audit_history(&stream).await?;
                if u64::try_from(events.len())? != options.events {
                    return Err("fixture audit count differs".into());
                }
                println!("{}", json!({"action":"audit","events":events.len()}));
            }
            Action::Capture => {
                let started = Instant::now();
                let snapshot = executor
                    .create_snapshot::<model::InventoryAggregate>(
                        &stream,
                        StreamVersion::new(options.events),
                    )
                    .await?;
                let elapsed = started.elapsed().as_nanos();
                let state =
                    <JsonSnapshotCodec<model::InventoryAggregate> as SnapshotCodec<
                        model::InventoryAggregate,
                    >>::decode(&JsonSnapshotCodec::default(), &snapshot.state)?;
                model::verify(&state, options.events, NOTE_BYTES)?;
                println!(
                    "{}",
                    json!({"action":"capture","scenario":options.scenario,"events":options.events,"elapsed_ns":elapsed,"state_bytes":snapshot.state.len(),"envelope_bytes":serde_json::to_vec(&snapshot)?.len(),"rebuild":diagnostics.recent().last()})
                );
            }
            Action::Read | Action::Command => {
                let mut measurements = Vec::new();
                let mut head = options.events;
                for iteration in 0..options.warmups.saturating_add(options.samples) {
                    connection.client().flush().await?;
                    let counters = traffic(&connection);
                    let started = Instant::now();
                    if matches!(options.action, Action::Read) {
                        let aggregate = executor
                            .rehydrate::<model::InventoryAggregate>(&stream)
                            .await?;
                        let elapsed_ns = started.elapsed().as_nanos();
                        model::verify(aggregate.state(), head, NOTE_BYTES)?;
                        if iteration >= options.warmups {
                            measurements.push(sample(
                                &connection,
                                counters,
                                elapsed_ns,
                                head,
                                diagnostics.recent().last(),
                            )?);
                        }
                    } else {
                        let command = seed::command(ID, head, NOTE_BYTES)?;
                        let metadata = model::metadata(
                            &format!("scaling-{}-{head}", options.scenario),
                            &command,
                        )?;
                        seed::appended(
                            executor
                                .execute(&model::ReceiveStockHandler, metadata, &command)
                                .await?,
                        )?;
                        let elapsed_ns = started.elapsed().as_nanos();
                        if iteration >= options.warmups {
                            measurements.push(sample(
                                &connection,
                                counters,
                                elapsed_ns,
                                head,
                                diagnostics.recent().last(),
                            )?);
                        }
                        head = head.saturating_add(1);
                    }
                }
                if matches!(options.action, Action::Command) {
                    let aggregate = executor
                        .rehydrate::<model::InventoryAggregate>(&stream)
                        .await?;
                    model::verify(aggregate.state(), head, NOTE_BYTES)?;
                }
                println!(
                    "{}",
                    json!({"action":format!("{:?}",options.action).to_lowercase(),"scenario":options.scenario,"events":options.events,"head_after":head,"samples":measurements})
                );
            }
            _ => return Err("unsupported measurement action".into()),
        }
    }
    connection.drain().await?;
    Ok(())
}

fn traffic(connection: &NatsConnection) -> (u64, u64, u64) {
    let stats = connection.client().statistics();
    (
        stats.in_messages.load(Ordering::Relaxed),
        stats.in_bytes.load(Ordering::Relaxed),
        stats.out_messages.load(Ordering::Relaxed),
    )
}
fn sample(
    connection: &NatsConnection,
    before: (u64, u64, u64),
    elapsed_ns: u128,
    head: u64,
    rebuild: Option<&rostfrei_core::RebuildReport>,
) -> BenchResult<Value> {
    let after = traffic(connection);
    Ok(
        json!({"elapsed_ns":u64::try_from(elapsed_ns)?,"head":head,"in_messages":after.0.saturating_sub(before.0),"in_bytes":after.1.saturating_sub(before.1),"out_messages":after.2.saturating_sub(before.2),"rebuild":rebuild}),
    )
}

/// Narrow schema-3 direct-event import fixture, using the production wire's exact
/// field order/checksum and deterministic identities. No receipts are fabricated.
/// Production audit/rehydration checks the imported fixture before measurements.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportEvent<'a> {
    event_store_stream: &'a str,
    application: &'a str,
    bounded_context: &'a str,
    stream: ImportStream<'a>,
    stream_version: u64,
    commit_id: &'a str,
    operation_id: &'a str,
    operation_fingerprint: &'a str,
    commit_event_ordinal: u32,
    commit_event_count: u32,
    event_id: &'a str,
    event_type: &'a str,
    event_schema_version: u32,
    payload_base64: &'a str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportStream<'a> {
    aggregate_type: &'a str,
    aggregate_id: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Checksum<'a, 'b> {
    schema_version: u16,
    event: &'a ImportEvent<'b>,
}

async fn import_fixture(
    connection: &NatsConnection,
    config: &NatsEventStoreConfig,
    start: u64,
    end: u64,
) -> BenchResult {
    let stream_id = model::stream(ID)?;
    let subject = config.aggregate_subject(stream_id.aggregate_type().as_str(), ID);
    let stream = connection
        .jetstream()
        .get_stream(config.stream_name())
        .await?;
    let mut predecessor = if start == 0 {
        0
    } else {
        stream
            .get_last_raw_message_by_subject(&subject)
            .await?
            .sequence
    };
    let payloads: Vec<_> = (0..64)
        .map(|index| {
            Ok(STANDARD.encode(model::StockReceived::at(index, NOTE_BYTES)?.encode_json()?))
        })
        .collect::<BenchResult<_>>()?;
    let reply = connection.client().new_inbox();
    let mut responses = connection.client().subscribe(reply.clone()).await?;
    let started = Instant::now();
    let mut position = start;
    while position < end {
        let count = u32::try_from(end.saturating_sub(position).min(100))?;
        let operation = OperationId::new(format!("bulk-{position}"))?;
        let commit = derive_commit_id(&stream_id, &operation);
        let fingerprint = ContentFingerprint::digest(operation.as_str()).to_hex();
        let batch = ContentFingerprint::digest(connection.client().new_inbox()).to_hex();
        for ordinal in 0..count {
            let index = position.saturating_add(u64::from(ordinal));
            let event_id = derive_event_id(&commit, ordinal);
            let event = ImportEvent {
                event_store_stream: config.stream_name(),
                application: config.application().as_str(),
                bounded_context: config.bounded_context().as_str(),
                stream: ImportStream {
                    aggregate_type: stream_id.aggregate_type().as_str(),
                    aggregate_id: ID,
                },
                stream_version: index.saturating_add(1),
                commit_id: commit.as_str(),
                operation_id: operation.as_str(),
                operation_fingerprint: &fingerprint,
                commit_event_ordinal: ordinal,
                commit_event_count: count,
                event_id: event_id.as_str(),
                event_type: "stock-received",
                event_schema_version: 1,
                payload_base64: payloads
                    .get(usize::try_from(index.rem_euclid(64))?)
                    .ok_or("missing SKU")?,
            };
            let checksum = ContentFingerprint::digest(serde_json::to_vec(&Checksum {
                schema_version: 3,
                event: &event,
            })?)
            .to_hex();
            let payload =
                serde_json::to_vec(&json!({"schemaVersion":3,"checksum":checksum,"event":event}))?;
            let mut headers = async_nats::HeaderMap::new();
            headers.insert("Content-Type", "application/json");
            headers.insert("Nats-Required-Api-Level", "2");
            headers.insert("Nats-Batch-Id", batch.as_str());
            headers.insert("Nats-Batch-Sequence", ordinal.saturating_add(1).to_string());
            headers.insert("Nats-Msg-Id", event_id.as_str());
            if ordinal == 0 {
                headers.insert("Nats-Expected-Stream", config.stream_name());
                headers.insert(
                    "Nats-Expected-Last-Subject-Sequence",
                    predecessor.to_string(),
                );
            }
            if ordinal.saturating_add(1) == count {
                headers.insert("Nats-Batch-Commit", "1");
            }
            connection
                .client()
                .publish_with_reply_and_headers(
                    subject.clone(),
                    reply.clone(),
                    headers,
                    payload.into(),
                )
                .await?;
        }
        for ordinal in 0..count {
            let response = tokio::time::timeout(Duration::from_secs(30), responses.next())
                .await?
                .ok_or("import response subscription ended")?;
            if ordinal.saturating_add(1) == count {
                let ack: Value = serde_json::from_slice(&response.payload)?;
                if ack.get("error").is_some() {
                    return Err(format!("fixture publish: {ack}").into());
                }
                predecessor = ack
                    .get("seq")
                    .and_then(Value::as_u64)
                    .ok_or("missing fixture ack sequence")?;
            } else if !response.payload.is_empty() {
                return Err(format!("unexpected nonfinal ack: {:?}", response.payload).into());
            }
        }
        position = position.saturating_add(u64::from(count));
    }
    connection.client().flush().await?;
    println!(
        "{}",
        json!({"action":"seed","start":start,"events":end,"elapsed_ns":started.elapsed().as_nanos()})
    );
    Ok(())
}
