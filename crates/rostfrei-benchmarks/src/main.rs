use std::{
    ffi::OsString,
    path::PathBuf,
    process::{ExitCode, Stdio},
    time::Duration,
};

use clap::{Parser, Subcommand};
use rostfrei_benchmarks::{BenchResult, broker::Broker, comparison, process};
use tokio::process::Command;

#[derive(Parser)]
#[command(about = "Rust-only benchmark workloads and pinned disposable NATS runner")]
struct Options {
    /// Use an explicitly selected existing broker instead of creating a disposable one.
    #[arg(long)]
    nats_url: Option<String>,
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=300))]
    ready_timeout_seconds: u64,
    #[command(subcommand)]
    command: Mode,
}

#[derive(Subcommand)]
enum Mode {
    /// Compare saved before/after release read-model benchmark executables.
    CompareHistory(comparison::Options),
    /// Run the read-model query benchmark; forward options after --.
    ReadModel {
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Run history loading, full aggregate readiness and executor write benchmarks.
    AggregateLoading {
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Run a command against a disposable broker (also useful for Rust integration tests).
    Run {
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
}

#[tokio::main(worker_threads = 2)]
async fn main() -> ExitCode {
    let options = Options::parse();
    let result = tokio::select! {
        result = execute(options) => result,
        signal = cancelled() => signal.map(ExitCode::from),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Benchmark runner failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn execute(options: Options) -> BenchResult<ExitCode> {
    let broker = if options.nats_url.is_none() {
        Some(Broker::start(Duration::from_secs(options.ready_timeout_seconds)).await?)
    } else {
        None
    };
    let url = options
        .nats_url
        .as_deref()
        .or_else(|| broker.as_ref().map(Broker::url))
        .ok_or("missing broker URL")?;
    if url.trim().is_empty() {
        return Err("NATS URL must not be empty".into());
    }
    let result = match options.command {
        Mode::CompareHistory(compare) => comparison::run(&compare, url).await.map(|()| 0),
        Mode::ReadModel { args } => {
            child(
                vec![sibling("read-model-benchmark")?.into_os_string()],
                args,
                url,
            )
            .await
        }
        Mode::AggregateLoading { args } => {
            child(
                vec![sibling("aggregate-loading")?.into_os_string()],
                args,
                url,
            )
            .await
        }
        Mode::Run { command } => child(command, Vec::new(), url).await,
    };
    if let Some(broker) = broker {
        if result.as_ref().map_or(true, |code| *code != 0) {
            broker.logs().await;
        }
        let cleanup = broker.shutdown().await;
        let code = result?;
        cleanup?;
        Ok(ExitCode::from(code))
    } else {
        result.map(ExitCode::from)
    }
}

fn sibling(name: &str) -> BenchResult<PathBuf> {
    let path =
        std::env::current_exe()?.with_file_name(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        return Err(
            "build all workloads with cargo build --release -p rostfrei-benchmarks --bins".into(),
        );
    }
    Ok(path)
}

async fn child(command: Vec<OsString>, args: Vec<OsString>, url: &str) -> BenchResult<u8> {
    use std::io::Write as _;
    let (program, remaining) = command.split_first().ok_or("missing child command")?;
    let mut child = Command::new(program);
    child
        .args(remaining)
        .args(args)
        .env("ROSTFREI_NATS_URL", url)
        .env("ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES", "67108864")
        .env("ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES", "268435456")
        .env("ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES", "524288")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let result = process::output(&mut child, Duration::from_secs(3600)).await?;
    std::io::stdout().write_all(&result.stdout)?;
    if let Some(code) = result.status.code() {
        return Ok(u8::try_from(code)?);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        let signal = result
            .status
            .signal()
            .ok_or("child has no exit code or signal")?;
        Ok(u8::try_from(
            128_i32.checked_add(signal).ok_or("exit status overflow")?,
        )?)
    }
    #[cfg(not(unix))]
    Err("child terminated without an exit code".into())
}

async fn cancelled() -> BenchResult<u8> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            interrupt = tokio::signal::ctrl_c() => { interrupt?; Ok(130) },
            _ = terminate.recv() => Ok(143),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok(130)
    }
}
