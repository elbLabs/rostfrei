use std::{sync::atomic::Ordering, time::Duration};

use async_nats::Client;
use serde::Serialize;

use super::BenchResult;

#[derive(Clone, Copy)]
pub struct Traffic {
    sent: u64,
    received_bytes: u64,
}

impl Traffic {
    pub fn capture(client: &Client) -> Self {
        let stats = client.statistics();
        Self {
            sent: stats.out_messages.load(Ordering::Relaxed),
            received_bytes: stats.in_bytes.load(Ordering::Relaxed),
        }
    }
}

#[derive(Default)]
pub struct Samples {
    durations: Vec<Duration>,
    sent: u64,
    received_bytes: u64,
}

#[derive(Serialize)]
pub struct Summary {
    pub path: &'static str,
    pub samples: u32,
    pub mean_us: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    /// Reciprocal mean service time at concurrency one, not saturation QPS.
    pub serial_queries_per_second: f64,
    pub published_messages_per_query: f64,
    pub received_bytes_per_query: f64,
}

impl Samples {
    pub fn record(&mut self, elapsed: Duration, before: Traffic, after: Traffic) -> BenchResult {
        self.durations.push(elapsed);
        self.sent = self
            .sent
            .checked_add(
                after
                    .sent
                    .checked_sub(before.sent)
                    .ok_or("message counter regressed")?,
            )
            .ok_or("message count overflow")?;
        self.received_bytes = self
            .received_bytes
            .checked_add(
                after
                    .received_bytes
                    .checked_sub(before.received_bytes)
                    .ok_or("byte counter regressed")?,
            )
            .ok_or("byte count overflow")?;
        Ok(())
    }

    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_precision_loss,
        clippy::as_conversions
    )]
    pub fn summarize(mut self, path: &'static str) -> BenchResult<Summary> {
        let samples = u32::try_from(self.durations.len())?;
        if samples == 0 {
            return Err("no benchmark samples".into());
        }
        let total: Duration = self.durations.iter().sum();
        if total.is_zero() {
            return Err("timer resolution insufficient".into());
        }
        self.durations.sort_unstable();
        let count = f64::from(samples);
        Ok(Summary {
            path,
            samples,
            mean_us: total.as_secs_f64() * 1_000_000.0 / count,
            p50_us: percentile(&self.durations, 50)?.as_secs_f64() * 1_000_000.0,
            p95_us: percentile(&self.durations, 95)?.as_secs_f64() * 1_000_000.0,
            p99_us: percentile(&self.durations, 99)?.as_secs_f64() * 1_000_000.0,
            serial_queries_per_second: count / total.as_secs_f64(),
            published_messages_per_query: self.sent as f64 / count,
            received_bytes_per_query: self.received_bytes as f64 / count,
        })
    }
}

fn percentile(sorted: &[Duration], percent: usize) -> BenchResult<Duration> {
    let rank = sorted
        .len()
        .checked_mul(percent)
        .ok_or("percentile rank overflow")?
        .div_ceil(100);
    sorted
        .get(rank.saturating_sub(1))
        .copied()
        .ok_or_else(|| "empty percentile sample".into())
}
