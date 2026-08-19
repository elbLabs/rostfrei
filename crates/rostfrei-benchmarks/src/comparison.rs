use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use clap::{Parser, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use tokio::process::Command;

use crate::{BenchResult, process};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Workload {
    SingleEventCommits,
    LongHistories,
    Transactional,
    TransactionalLong,
    TransactionalAudited,
}

impl Workload {
    const fn name(self) -> &'static str {
        match self {
            Self::SingleEventCommits => "single_event_commits",
            Self::LongHistories => "long_histories",
            Self::Transactional => "transactional",
            Self::TransactionalLong => "transactional_long",
            Self::TransactionalAudited => "transactional_audited",
        }
    }

    fn arguments(self, options: &Options) -> Vec<String> {
        let long = matches!(self, Self::LongHistories | Self::TransactionalLong);
        let counts = if options.events_per_aggregate.is_empty() {
            if long {
                "500,1000".to_owned()
            } else {
                "10,50,100".to_owned()
            }
        } else {
            options
                .events_per_aggregate
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        let samples = if options.quick {
            2
        } else if long {
            20
        } else if matches!(self, Self::SingleEventCommits) {
            100
        } else {
            30
        };
        let rounds = if options.quick {
            1
        } else if matches!(self, Self::SingleEventCommits) {
            3
        } else {
            2
        };
        let warmup = if options.quick {
            0
        } else if matches!(self, Self::SingleEventCommits) {
            10
        } else {
            5
        };
        let mut args = vec![
            "--events-per-aggregate".to_owned(),
            counts,
            "--samples".to_owned(),
            samples.to_string(),
            "--rounds".to_owned(),
            rounds.to_string(),
            "--warmup".to_owned(),
            warmup.to_string(),
        ];
        if matches!(self, Self::LongHistories) {
            args.extend(["--events-per-commit".to_owned(), "99".to_owned()]);
        }
        if matches!(
            self,
            Self::Transactional | Self::TransactionalLong | Self::TransactionalAudited
        ) {
            args.push("--transactional".to_owned());
        }
        if matches!(self, Self::TransactionalAudited) {
            args.push("--history-auditing".to_owned());
        }
        args
    }
}

#[derive(Debug, Parser)]
pub struct Options {
    #[arg(long)]
    pub before: PathBuf,
    #[arg(long)]
    pub after: PathBuf,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long)]
    pub baseline_revision: String,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        default_value = "single-event-commits,long-histories,transactional,transactional-long,transactional-audited"
    )]
    pub workload: Vec<Workload>,
    /// Smoke validation only, not publishable latency percentiles.
    #[arg(long)]
    pub quick: bool,
    #[arg(long, value_delimiter = ',', value_parser = clap::value_parser!(u32).range(2..=1000))]
    pub events_per_aggregate: Vec<u32>,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub total_events: u64,
    pub path: String,
    pub p50_speedup: f64,
    pub before_p50_us: f64,
    pub after_p50_us: f64,
    pub before_p95_us: f64,
    pub after_p95_us: f64,
    pub before_requests: f64,
    pub after_requests: f64,
}

fn field<'a>(value: &'a Value, name: &str) -> BenchResult<&'a Value> {
    value
        .get(name)
        .ok_or_else(|| format!("benchmark field {name} is missing").into())
}

fn equal(before: &Value, after: &Value, names: &[&str]) -> BenchResult {
    for &name in names {
        if field(before, name)? != field(after, name)? {
            return Err(format!("benchmark {name} differs").into());
        }
    }
    Ok(())
}

fn paths(case: &Value) -> BenchResult<BTreeMap<&str, &Value>> {
    let results = field(case, "results")?
        .as_array()
        .ok_or("benchmark results must be an array")?;
    let mut paths = BTreeMap::new();
    for result in results {
        let name = field(result, "path")?
            .as_str()
            .ok_or("benchmark path must be text")?;
        if paths.insert(name, result).is_some() {
            return Err("duplicate benchmark path".into());
        }
    }
    if paths.is_empty() {
        return Err("benchmark has no paths".into());
    }
    Ok(paths)
}

fn metric(value: &Value, name: &str) -> BenchResult<f64> {
    let number = field(value, name)?
        .as_f64()
        .ok_or("benchmark metric must be numeric")?;
    if !number.is_finite() || number < 0.0 {
        return Err("benchmark metric is invalid".into());
    }
    Ok(number)
}

/// Refuse mismatched read policies, work, environments, payloads or sample counts.
pub fn compare(before: &Value, after: &Value) -> BenchResult<Vec<Comparison>> {
    if field(before, "release_build")? != &Value::Bool(true)
        || field(after, "release_build")? != &Value::Bool(true)
    {
        return Err("both benchmark binaries must be release builds".into());
    }
    equal(
        before,
        after,
        &[
            "options",
            "nats_server_version",
            "runtime_workers",
            "architecture",
            "os",
        ],
    )?;
    let old_cases = field(before, "cases")?
        .as_array()
        .ok_or("benchmark cases must be an array")?;
    let new_cases = field(after, "cases")?
        .as_array()
        .ok_or("benchmark cases must be an array")?;
    if old_cases.is_empty() || old_cases.len() != new_cases.len() {
        return Err("benchmark case counts differ or are empty".into());
    }
    let mut comparisons = Vec::new();
    for (old, new) in old_cases.iter().zip(new_cases) {
        equal(
            old,
            new,
            &[
                "events_per_aggregate",
                "total_events",
                "source_payload_bytes",
                "encoded_snapshot_bytes",
                "encoded_response_bytes",
            ],
        )?;
        let old_paths = paths(old)?;
        let new_paths = paths(new)?;
        if old_paths.keys().ne(new_paths.keys()) {
            return Err("benchmark paths differ".into());
        }
        for (path, old_result) in old_paths {
            let new_result = new_paths.get(path).ok_or("benchmark path is missing")?;
            equal(old_result, new_result, &["samples"])?;
            if field(old_result, "samples")?
                .as_u64()
                .is_none_or(|samples| samples == 0)
            {
                return Err("benchmark sample count must be positive".into());
            }
            let before_p50_us = metric(old_result, "p50_us")?;
            let after_p50_us = metric(new_result, "p50_us")?;
            if before_p50_us <= 0.0 || after_p50_us <= 0.0 {
                return Err("benchmark median must be positive".into());
            }
            let p50_speedup = before_p50_us / after_p50_us;
            if !p50_speedup.is_finite() {
                return Err("benchmark speedup is not finite".into());
            }
            comparisons.push(Comparison {
                total_events: field(old, "total_events")?
                    .as_u64()
                    .ok_or("event count must be unsigned")?,
                path: path.to_owned(),
                p50_speedup,
                before_p50_us,
                after_p50_us,
                before_p95_us: metric(old_result, "p95_us")?,
                after_p95_us: metric(new_result, "p95_us")?,
                before_requests: metric(old_result, "published_messages_per_query")?,
                after_requests: metric(new_result, "published_messages_per_query")?,
            });
        }
    }
    Ok(comparisons)
}

async fn measure(binary: &Path, args: &[String], url: &str) -> BenchResult<Value> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env("ROSTFREI_NATS_URL", url)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let output = process::output(&mut command, Duration::from_secs(3600)).await?;
    if !output.status.success() {
        return Err(format!("benchmark {} failed: {}", binary.display(), output.status).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub async fn run(options: &Options, url: &str) -> BenchResult {
    let before = options.before.canonicalize()?;
    let after = options.after.canonicalize()?;
    if before == after {
        return Err("before and after must be different executable paths".into());
    }
    let mut report = json!({
        "baseline_revision": options.baseline_revision,
        "recorded_at": time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?,
        "binary_sha256": {"before": format!("{:x}", Sha256::digest(std::fs::read(&before)?)), "after": format!("{:x}", Sha256::digest(std::fs::read(&after)?))},
        "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "logical_cpus": std::thread::available_parallelism()?.get(),
        "quick": options.quick,
        "workloads": [],
    });
    for (index, workload) in options.workload.iter().copied().enumerate() {
        let args = workload.arguments(options);
        let order = if index.is_multiple_of(2) {
            ["before", "after"]
        } else {
            ["after", "before"]
        };
        let mut results = BTreeMap::new();
        for version in order {
            eprintln!("Running {}: {version}", workload.name());
            let binary = if version == "before" { &before } else { &after };
            results.insert(version, measure(binary, &args, url).await?);
        }
        let old = results.remove("before").ok_or("missing before report")?;
        let new = results.remove("after").ok_or("missing after report")?;
        let comparison = compare(&old, &new)?;
        let workloads = report
            .get_mut("workloads")
            .and_then(Value::as_array_mut)
            .ok_or("missing workloads")?;
        workloads.push(json!({"name": workload.name(), "execution_order": order, "before": old, "after": new, "comparison": comparison}));
        write(&report, &options.output)?;
        for result in comparison
            .iter()
            .filter(|result| result.path == "replay_sequential")
        {
            eprintln!(
                "{}, {} events: {:.3} -> {:.3} ms ({:.2}x), requests {} -> {}",
                workload.name(),
                result.total_events,
                result.before_p50_us / 1000.0,
                result.after_p50_us / 1000.0,
                result.p50_speedup,
                result.before_requests,
                result.after_requests
            );
        }
    }
    Ok(())
}

fn write(report: &Value, output: &Path) -> BenchResult {
    use std::io::Write as _;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(file.as_file_mut(), report)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(output)?;
    Ok(())
}
