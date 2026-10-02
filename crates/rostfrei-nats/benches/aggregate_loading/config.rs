use std::{collections::HashSet, path::PathBuf};

use clap::{Parser, ValueEnum};
use serde::Serialize;

use super::BenchResult;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum HistoryKind {
    Direct,
    Commands,
}

impl HistoryKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Commands => "commands",
        }
    }
}

#[derive(Clone, Debug, Parser, Serialize)]
#[command(about = "Benchmark validated history, aggregate readiness, apply, and executor writes")]
pub struct Options {
    #[arg(long, value_delimiter = ',', default_value = "0,100,1000")]
    pub events: Vec<u32>,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        default_value = "direct,commands"
    )]
    pub history: Vec<HistoryKind>,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub samples: u32,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(0..=100))]
    pub warmups: u32,
    /// Bytes in the event's note field; total JSON/wire payload is larger.
    #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u32).range(0..=65536))]
    pub note_bytes: u32,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u32).range(1..=64))]
    pub workers: u32,
    #[arg(long, default_value_t = 1800, value_parser = clap::value_parser!(u64).range(1..=86400))]
    pub case_timeout_seconds: u64,
    #[arg(long, default_value_t = 268_435_456, value_parser = clap::value_parser!(i64).range(1..))]
    pub stream_bytes: i64,
    #[arg(long, default_value_os_t = default_output())]
    pub output: PathBuf,
    // Cargo's custom benchmark/test target invocation flags.
    #[arg(long, hide = true)]
    #[serde(skip)]
    pub bench: bool,
    #[arg(long, hide = true)]
    #[serde(skip)]
    pub test: bool,
}

fn default_output() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/benchmarks/aggregate-loading.json")
}

impl Options {
    pub fn validate(&self) -> BenchResult {
        if self.events.is_empty()
            || self.events.iter().any(|count| *count > 100_000)
            || self.events.iter().collect::<HashSet<_>>().len() != self.events.len()
        {
            return Err("--events requires unique counts between 0 and 100000".into());
        }
        if self.history.is_empty()
            || self.history.iter().collect::<HashSet<_>>().len() != self.history.len()
        {
            return Err("--history requires unique history kinds".into());
        }
        Ok(())
    }
}
