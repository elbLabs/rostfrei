use std::{path::Path, time::Duration};

use serde::Serialize;

use super::{BenchResult, Options, config::HistoryKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    HistoryLoad,
    Rehydrate,
    ApplyOnly,
    ExecuteCommand,
}

pub const READ_PHASES: [Phase; 3] = [Phase::HistoryLoad, Phase::Rehydrate, Phase::ApplyOnly];
const PHASES: [Phase; 4] = [
    Phase::HistoryLoad,
    Phase::Rehydrate,
    Phase::ApplyOnly,
    Phase::ExecuteCommand,
];

#[derive(Debug, Serialize)]
pub struct Sample {
    pub phase: Phase,
    pub iteration: u32,
    pub events_before: u64,
    pub elapsed_ns: u64,
    pub nats_out_messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_load_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_history_ns: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub phase: Phase,
    pub samples: usize,
    pub min_ns: u64,
    pub median_ns: u64,
    pub max_ns: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_ns: Option<u64>,
    pub min_out_messages: u64,
    pub max_out_messages: u64,
    pub min_events_before: u64,
    pub max_events_before: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_after_history_ns: Option<u64>,
}

#[derive(Serialize)]
pub struct CaseReport {
    pub history: HistoryKind,
    pub seeded_events: u32,
    pub seeded_domain_payload_bytes: usize,
    pub setup_ns: u64,
    pub samples: Vec<Sample>,
    pub summaries: Vec<Summary>,
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub started_at_unix_ms: u64,
    pub git_revision: Option<String>,
    pub git_dirty: Option<bool>,
    pub framework_version: &'static str,
    pub rustc: String,
    pub nats_server_version: String,
    pub operating_system: &'static str,
    pub architecture: &'static str,
    pub available_parallelism: Option<usize>,
    pub workload: &'static str,
    pub options: Options,
    pub cases: Vec<CaseReport>,
}

pub fn nanos(duration: Duration) -> BenchResult<u64> {
    Ok(u64::try_from(duration.as_nanos())?)
}

pub fn summarize(samples: &[Sample]) -> BenchResult<Vec<Summary>> {
    PHASES
        .into_iter()
        .map(|phase| {
            let selected: Vec<_> = samples
                .iter()
                .filter(|sample| sample.phase == phase)
                .collect();
            let sorted = |extract: fn(&Sample) -> u64| {
                let mut values: Vec<_> = selected.iter().map(|sample| extract(sample)).collect();
                values.sort_unstable();
                values
            };
            let elapsed = sorted(|sample| sample.elapsed_ns);
            let messages = sorted(|sample| sample.nats_out_messages);
            let events = sorted(|sample| sample.events_before);
            let mut after: Vec<_> = selected
                .iter()
                .filter_map(|sample| sample.after_history_ns)
                .collect();
            after.sort_unstable();
            let p95_ns = if elapsed.len() >= 20 {
                let rank = elapsed
                    .len()
                    .checked_mul(95)
                    .map(|value| value.div_ceil(100))
                    .and_then(|value| value.checked_sub(1))
                    .ok_or("percentile index overflow")?;
                elapsed.get(rank).copied()
            } else {
                None
            };
            Ok(Summary {
                phase,
                samples: selected.len(),
                min_ns: *elapsed.first().ok_or("phase has no samples")?,
                median_ns: median(&elapsed)?,
                max_ns: *elapsed.last().ok_or("phase has no samples")?,
                p95_ns,
                min_out_messages: *messages.first().ok_or("phase has no counters")?,
                max_out_messages: *messages.last().ok_or("phase has no counters")?,
                min_events_before: *events.first().ok_or("phase has no event counts")?,
                max_events_before: *events.last().ok_or("phase has no event counts")?,
                median_after_history_ns: if after.is_empty() {
                    None
                } else {
                    Some(median(&after)?)
                },
            })
        })
        .collect()
}

fn median(sorted: &[u64]) -> BenchResult<u64> {
    let middle = sorted.len().checked_div(2).ok_or("median index overflow")?;
    let upper = *sorted.get(middle).ok_or("cannot summarize empty samples")?;
    if !sorted.len().is_multiple_of(2) {
        return Ok(upper);
    }
    let lower = *sorted
        .get(middle.saturating_sub(1))
        .ok_or("missing lower median")?;
    lower
        .checked_add(upper)
        .and_then(|sum| sum.checked_div(2))
        .ok_or_else(|| "median overflow".into())
}

pub fn write(report: &Report, output: &Path) -> BenchResult {
    use std::io::Write as _;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(file.as_file_mut(), report)?;
    file.as_file_mut().write_all(b"\n")?;
    file.as_file_mut().sync_all()?;
    file.persist(output)?;
    Ok(())
}
