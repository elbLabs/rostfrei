use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
    process::Command,
};

use crate::{BenchResult, process};

pub const IMAGE: &str =
    "nats:2.12.1-alpine@sha256:b3f2bd84176ae7bd0afa9c48a00f06d7d0818ff4aaee898e4172e0b8340e5816";
const DOCKER_TIMEOUT: Duration = Duration::from_secs(30);
const CONFIG: &str = "port: 4222\nhttp_port: 8222\nmax_payload: 2113536\njetstream {\n  store_dir: \"/data/jetstream\"\n}\n";

/// Owns only its uniquely named, loopback-bound disposable container.
pub struct Broker {
    name: String,
    url: String,
    armed: bool,
    config: tempfile::NamedTempFile,
}

impl Broker {
    pub async fn start(ready_timeout: Duration) -> BenchResult<Self> {
        use std::io::Write as _;
        if ready_timeout.is_zero() {
            return Err("broker readiness timeout must be positive".into());
        }
        docker(&["info"], DOCKER_TIMEOUT).await?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let mut config = tempfile::NamedTempFile::new()?;
        config.write_all(CONFIG.as_bytes())?;
        config.as_file().sync_all()?;
        let mut broker = Self {
            name: format!("rostfrei-benchmark-nats-{}-{nonce}", std::process::id()),
            url: String::new(),
            armed: true,
            config,
        };
        // Arm before creation, including cancellation/timeout after Docker accepted the run.
        let mount = format!(
            "type=bind,src={},dst=/etc/nats/benchmark.conf,readonly",
            broker.config.path().display()
        );
        docker(
            &[
                "run",
                "--detach",
                "--rm",
                "--name",
                &broker.name,
                "--publish",
                "127.0.0.1::4222",
                "--publish",
                "127.0.0.1::8222",
                "--mount",
                &mount,
                IMAGE,
                "--config",
                "/etc/nats/benchmark.conf",
            ],
            Duration::from_secs(240),
        )
        .await?;
        let ports = docker(
            &[
                "inspect",
                "--format",
                "{{json .NetworkSettings.Ports}}",
                &broker.name,
            ],
            DOCKER_TIMEOUT,
        )
        .await?;
        let ports: Value = serde_json::from_slice(&ports)?;
        let nats = mapped_port(&ports, "4222/tcp")?;
        let monitor = mapped_port(&ports, "8222/tcp")?;
        broker.url = format!("nats://127.0.0.1:{nats}");
        let ready = tokio::time::timeout(ready_timeout, async {
            loop {
                if health(monitor).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        })
        .await;
        if ready.is_err() {
            broker.logs().await;
            return Err(
                format!("NATS JetStream did not become ready within {ready_timeout:?}").into(),
            );
        }
        eprintln!("NATS 2.12.1 ready on isolated port {nats}");
        Ok(broker)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn logs(&self) {
        match docker(&["logs", &self.name], DOCKER_TIMEOUT).await {
            Ok(logs) => eprintln!("{}", String::from_utf8_lossy(&logs)),
            Err(error) => eprintln!("Could not retrieve NATS logs: {error}"),
        }
    }

    /// A cleanup failure must not turn into a successful benchmark run.
    pub async fn shutdown(mut self) -> BenchResult {
        docker(&["rm", "--force", "--volumes", &self.name], DOCKER_TIMEOUT).await?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Drop also runs on cancelled futures. Bound the fallback even during runtime loss.
        let spawned = std::process::Command::new("docker")
            .args(["rm", "--force", "--volumes", &self.name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Ok(mut child) = spawned {
            let started = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            eprintln!("Best-effort NATS cleanup failed for {}", self.name);
                        }
                        break;
                    }
                    Ok(None) if started.elapsed() < DOCKER_TIMEOUT => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }
}

async fn docker(args: &[&str], timeout: Duration) -> BenchResult<Vec<u8>> {
    let mut command = Command::new("docker");
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = process::output(&mut command, timeout).await?;
    if !output.status.success() {
        return Err(format!("Docker failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    let mut bytes = output.stdout;
    if args.first() == Some(&"logs") {
        bytes.extend(output.stderr);
    }
    Ok(bytes)
}

fn mapped_port(ports: &Value, key: &str) -> BenchResult<u16> {
    let mappings = ports
        .get(key)
        .and_then(Value::as_array)
        .ok_or("missing Docker port mapping")?;
    if mappings.len() != 1 {
        return Err("NATS ports must have exactly one loopback binding".into());
    }
    let mapping = mappings.first().ok_or("missing Docker port mapping")?;
    if mapping.get("HostIp").and_then(Value::as_str) != Some("127.0.0.1") {
        return Err("NATS ports must be published only on loopback".into());
    }
    let port = mapping
        .get("HostPort")
        .and_then(Value::as_str)
        .ok_or("missing Docker host port")?
        .parse::<u16>()?;
    if port == 0 {
        return Err("Docker host port must be nonzero".into());
    }
    Ok(port)
}

async fn health(port: u16) -> BenchResult {
    tokio::time::timeout(Duration::from_secs(1), async {
        let mut socket = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await?;
        socket.write_all(b"GET /healthz?js-enabled-only=true HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n").await?;
        let mut response = Vec::new();
        socket.take(16 * 1024).read_to_end(&mut response).await?;
        let response = String::from_utf8(response)?;
        let (headers, body) = response.split_once("\r\n\r\n").ok_or("invalid health response")?;
        if !headers.lines().next().is_some_and(|status| status.starts_with("HTTP/1.0 200 ") || status.starts_with("HTTP/1.1 200 ")) {
            return Err("NATS health endpoint is not ready".into());
        }
        let body: Value = serde_json::from_str(body)?;
        if body.get("status").and_then(Value::as_str) != Some("ok") { return Err("JetStream is not healthy".into()); }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_bindings_reject_public_missing_multiple_and_zero_ports() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"port": []}),
            serde_json::json!({"port": [{"HostIp":"0.0.0.0","HostPort":"1234"}]}),
            serde_json::json!({"port": [{"HostIp":"127.0.0.1","HostPort":"0"}]}),
            serde_json::json!({"port": [{"HostIp":"127.0.0.1","HostPort":"70000"}]}),
            serde_json::json!({"port": [{},{}]}),
        ] {
            assert!(mapped_port(&value, "port").is_err());
        }
        assert_eq!(
            mapped_port(
                &serde_json::json!({"port":[{"HostIp":"127.0.0.1","HostPort":"1234"}]}),
                "port"
            )
            .ok(),
            Some(1234)
        );
    }
}
