#![allow(
    clippy::panic_in_result_fn,
    reason = "benchmark assertions check equivalent aggregate results"
)]

use rostfrei_benchmarks::aggregate_loading as suite;

use std::{
    sync::atomic::Ordering,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use rostfrei_core::CommandExecutor;
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{
    NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, connect, provision_event_store,
};
use suite::{
    BenchResult,
    config::HistoryKind,
    model::{self, InventoryAggregate},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "release-mode A/B benchmark of 1000 genuine command transactions"]
async fn compare_trusted_and_audited_aggregate_readiness() -> BenchResult {
    tokio::spawn(compare_readiness()).await?
}

async fn compare_readiness() -> BenchResult {
    let url = rostfrei_testing::integration::nats_url()?;
    let connection = connect(&NatsConnectionConfig::new("history-audit-benchmark", url)).await?;
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let app = ApplicationName::new(format!("audit-bench-{}-{unique}", std::process::id()))?;
    let config =
        NatsEventStoreConfig::for_bounded_context(&app.test_bounded_context(model::CONTEXT)?)?
            .with_storage_limits(64 * 1024 * 1024, 512 * 1024)?;
    let result: BenchResult = async {
        provision_event_store(connection.jetstream(), &config).await?;
        let store = NatsEventStore::connect(connection.jetstream().clone(), config.clone()).await?;
        suite::seed::seed(store.clone(), "inventory", HistoryKind::Commands, 1000, 128).await?;
        let stream = model::stream("inventory")?;
        let trusted = CommandExecutor::new(store.clone());
        let audited = CommandExecutor::new(store.with_history_auditing(true));
        let expected = audited.rehydrate::<InventoryAggregate>(&stream).await?;
        model::verify(expected.state(), 1000, 128)?;
        assert_eq!(trusted.rehydrate::<InventoryAggregate>(&stream).await?.state(), expected.state());
        let client = connection.client();
        for block in 0..3 {
            for audit in [true, false, false, true] {
                client.flush().await?;
                let before = client.statistics().out_messages.load(Ordering::Relaxed);
                let started = Instant::now();
                let aggregate = if audit {
                    audited.rehydrate::<InventoryAggregate>(&stream).await?
                } else {
                    trusted.rehydrate::<InventoryAggregate>(&stream).await?
                };
                let elapsed = started.elapsed();
                model::verify(aggregate.state(), 1000, 128)?;
                assert_eq!(aggregate.state(), expected.state());
                client.flush().await?;
                let requests = client.statistics().out_messages.load(Ordering::Relaxed)
                    .checked_sub(before).ok_or("request counter regressed")?;
                let mode = if audit { "audited" } else { "trusted" };
                eprintln!("AUDIT_BENCH events=1000 block={block} mode={mode} requests={requests} elapsed_us={}", elapsed.as_micros());
            }
        }
        Ok(())
    }.await;
    let cleanup = connection
        .delete_stream_if_exists(config.stream_name())
        .await;
    let drain = connection.drain().await;
    result?;
    cleanup?;
    drain?;
    Ok(())
}
