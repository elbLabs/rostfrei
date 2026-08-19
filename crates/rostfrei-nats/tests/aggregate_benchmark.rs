#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions report benchmark contract failures"
)]

#[allow(
    dead_code,
    reason = "tests exercise benchmark contracts without starting its NATS runner"
)]
#[path = "../benches/aggregate_loading/mod.rs"]
mod suite;

use clap::Parser as _;
use suite::report::{Phase, Sample, summarize};

#[tokio::test]
async fn benchmark_fixtures_rehydrate_project_state_and_preserve_exact_replay() -> suite::BenchResult
{
    suite::smoke().await
}

#[tokio::test]
async fn large_fixture_payloads_cross_batch_boundaries_without_losing_events() -> suite::BenchResult
{
    use rostfrei_core::{CommandExecutor, InMemoryEventStore};
    let store = InMemoryEventStore::new();
    suite::seed::seed(
        store.clone(),
        "large-fixture",
        suite::config::HistoryKind::Direct,
        33,
        65536,
    )
    .await?;
    let stream = suite::model::stream("large-fixture")?;
    assert_eq!(store.load(&stream).await?.len(), 33);
    let aggregate = CommandExecutor::new(store)
        .rehydrate::<suite::model::InventoryAggregate>(&stream)
        .await?;
    suite::model::verify(aggregate.state(), 33, 65536)
}

#[test]
fn benchmark_options_reject_ambiguous_cases_and_zero_samples() {
    assert!(suite::Options::try_parse_from(["bench", "--samples", "0"]).is_err());
    assert!(suite::Options::try_parse_from(["bench", "--workers", "0"]).is_err());
    let duplicate = suite::Options::try_parse_from(["bench", "--events", "100,100"]).unwrap();
    assert!(duplicate.validate().is_err());
    let valid =
        suite::Options::try_parse_from(["bench", "--events", "0,1000", "--history", "commands"])
            .unwrap();
    assert!(valid.validate().is_ok());
}

#[test]
fn summaries_preserve_sample_counts_growth_and_quantile_definitions() -> suite::BenchResult {
    for count in [1_u32, 5, 20] {
        let mut samples = Vec::new();
        for phase in [
            Phase::HistoryLoad,
            Phase::Rehydrate,
            Phase::ApplyOnly,
            Phase::ExecuteCommand,
        ] {
            for index in 1..=count {
                samples.push(Sample {
                    phase,
                    iteration: index,
                    events_before: u64::from(index),
                    elapsed_ns: u64::from(index),
                    nats_out_messages: if phase == Phase::ApplyOnly { 0 } else { 3 },
                    history_load_ns: (phase == Phase::Rehydrate).then_some(0),
                    after_history_ns: (phase == Phase::Rehydrate).then_some(u64::from(index)),
                });
            }
        }
        for summary in summarize(&samples)? {
            assert_eq!(summary.samples, usize::try_from(count)?);
            assert_eq!(summary.min_ns, 1);
            assert_eq!(summary.max_ns, u64::from(count));
            assert_eq!(summary.min_events_before, 1);
            assert_eq!(summary.max_events_before, u64::from(count));
            assert_eq!(
                summary.median_ns,
                u64::from(
                    count
                        .saturating_add(1)
                        .checked_div(2)
                        .ok_or("median divisor")?
                )
            );
            assert_eq!(summary.p95_ns, (count == 20).then_some(19));
            if summary.phase == Phase::ApplyOnly {
                assert_eq!(summary.max_out_messages, 0);
            }
        }
    }
    assert!(summarize(&[]).is_err());
    Ok(())
}
