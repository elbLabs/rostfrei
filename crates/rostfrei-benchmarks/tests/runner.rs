#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions report runner lifecycle failures"
)]

use std::{process::Stdio, time::Duration};

use rostfrei_benchmarks::{BenchResult, broker::Broker, process};
use tokio::process::Command;

#[tokio::test]
async fn subprocess_status_and_output_are_preserved() -> BenchResult {
    let mut command = Command::new("sh");
    command
        .args(["-c", "printf test; printf diagnostic >&2; exit 7"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = process::output(&mut command, Duration::from_secs(5)).await?;
    assert_eq!(result.status.code(), Some(7));
    assert_eq!(result.stdout, b"test");
    assert_eq!(result.stderr, b"diagnostic");
    Ok(())
}

#[tokio::test]
async fn subprocess_waits_are_bounded() {
    let mut command = Command::new("sh");
    command
        .args(["-c", "sleep 60"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = std::time::Instant::now();
    assert!(
        process::output(&mut command, Duration::from_millis(50))
            .await
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn runner_propagates_child_failure_without_needing_a_broker() -> BenchResult {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rostfrei-benchmarks"));
    command
        .args([
            "--nats-url",
            "nats://127.0.0.1:1",
            "run",
            "--",
            "sh",
            "-c",
            "exit 7",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = process::output(&mut command, Duration::from_secs(5)).await?;
    assert_eq!(result.status.code(), Some(7));
    Ok(())
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn measurement_binaries_refuse_debug_timings_before_connecting() -> BenchResult {
    for binary in [
        env!("CARGO_BIN_EXE_aggregate-loading"),
        env!("CARGO_BIN_EXE_read-model-benchmark"),
    ] {
        let mut command = Command::new(binary);
        command
            .env_remove("ROSTFREI_NATS_URL")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let result = process::output(&mut command, Duration::from_secs(5)).await?;
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8(result.stderr)?.contains("--release"));
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn sigterm_cancels_the_runner_and_its_child_process_group() -> BenchResult {
    use tokio::io::{AsyncBufReadExt as _, BufReader};

    let mut runner = Command::new(env!("CARGO_BIN_EXE_rostfrei-benchmarks"))
        .args([
            "--nats-url",
            "nats://127.0.0.1:1",
            "run",
            "--",
            "sh",
            "-c",
            "echo $$ >&2; sleep 60",
        ])
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = runner.id().ok_or("runner PID")?;
    let stderr = runner.stderr.take().ok_or("runner stderr")?;
    let mut stderr = BufReader::new(stderr);
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), stderr.read_line(&mut line)).await??;
    let child_pid = line.trim().parse::<u32>()?;
    let mut terminate = Command::new("kill");
    terminate
        .args(["-TERM", &pid.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    assert!(
        process::output(&mut terminate, Duration::from_secs(5))
            .await?
            .status
            .success()
    );
    let result = tokio::time::timeout(Duration::from_secs(5), runner.wait_with_output()).await??;
    assert_eq!(result.status.code(), Some(143));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut alive = Command::new("kill");
            alive
                .args(["-0", &child_pid.to_string()])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if !process::output(&mut alive, Duration::from_secs(1))
                .await?
                .status
                .success()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker; opt-in real disposable broker lifecycle"]
async fn disposable_broker_is_healthy_and_cleanup_removes_only_its_container() -> BenchResult {
    let broker = Broker::start(Duration::from_secs(30)).await?;
    let name = broker.name().to_owned();
    let client = async_nats::connect(broker.url().to_owned()).await?;
    assert_eq!(client.server_info().version, "2.12.1");
    async_nats::jetstream::new(client).query_account().await?;
    broker.shutdown().await?;
    let mut inspect = Command::new("docker");
    inspect
        .args(["inspect", &name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    assert!(
        !process::output(&mut inspect, Duration::from_secs(10))
            .await?
            .status
            .success()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker; opt-in cancellation/drop cleanup"]
async fn dropping_a_replay_runner_removes_its_container() -> BenchResult {
    let broker = Broker::start(Duration::from_secs(30)).await?;
    let name = broker.name().to_owned();
    drop(broker);
    let mut inspect = Command::new("docker");
    inspect
        .args(["inspect", &name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    assert!(
        !process::output(&mut inspect, Duration::from_secs(10))
            .await?
            .status
            .success()
    );
    Ok(())
}
