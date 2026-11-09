use crate::{Environment, environment, nonce, process, root, save_json, versions};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

pub const IMAGE: &str =
    "nats:2.12.15-alpine@sha256:b270f5e2428354c0335612694d7dd2fb588148e567a5757fdff325ef9c9332e6";
pub const DEFAULT: &[&str] = &[
    "cargo",
    "test",
    "--locked",
    "--workspace",
    "--all-features",
    "--",
    "--test-threads=1",
];
pub fn limits(env: &mut Environment) {
    env.extend(
        [
            ("ROSTFREI_NATS_MESSAGING_STREAM_MAX_BYTES", "67108864"),
            ("ROSTFREI_NATS_EVENT_STORE_MAX_STREAM_BYTES", "268435456"),
            ("ROSTFREI_NATS_EVENT_STORE_MAX_EVENT_BYTES", "524288"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    );
    env.entry("RUST_BACKTRACE".into()).or_insert("1".into());
}
pub fn port() -> Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}
pub fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()?)
}
pub fn wait_ready(port: u16, timeout: Duration) -> Result<()> {
    let client = client(Duration::from_secs(1))?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        process::cancelled()?;
        if let Ok(response) = client
            .get(format!(
                "http://127.0.0.1:{port}/healthz?js-enabled-only=true"
            ))
            .send()
            && response.status().is_success()
            && let Ok(body) = response.text()
            && serde_json::from_str::<Value>(&body).is_ok_and(|v| v["status"] == "ok")
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("NATS JetStream did not become ready within {timeout:?}")
}
pub fn mapped_port(ports: &Value, key: &str) -> Result<u16> {
    let bindings = ports[key]
        .as_array()
        .context("missing Docker port mapping")?;
    ensure!(
        bindings.len() == 1 && bindings[0]["HostIp"] == "127.0.0.1",
        "NATS ports must have exactly one loopback binding"
    );
    let port = bindings[0]["HostPort"]
        .as_str()
        .context("missing host port")?
        .parse()?;
    ensure!(port != 0, "zero host port");
    Ok(port)
}
fn docker(args: &[&str], secrets: &[String]) -> Result<String> {
    let mut command = vec!["docker".to_owned()];
    command.extend(process::args(args));
    let output = process::capture(
        &command,
        &root(),
        &environment(),
        Duration::from_secs(if args.first() == Some(&"run") {
            240
        } else {
            30
        }),
        secrets,
        false,
    )?
    .checked()?;
    Ok(if args.first() == Some(&"logs") {
        format!("{}{}", output.stdout, output.stderr)
    } else {
        output.stdout
    })
}
pub struct Broker {
    name: String,
    armed: bool,
    pub env: Environment,
    pub secrets: Vec<String>,
    _private: tempfile::TempDir,
}
impl Broker {
    pub fn start(contracts: bool, ready_timeout: Duration) -> Result<Self> {
        let mut env = environment();
        limits(&mut env);
        let private = tempfile::Builder::new()
            .prefix("rostfrei-contract-")
            .tempdir()?;
        let mut config = fs::read_to_string(root().join("scripts/nats-test.conf"))?;
        let mut secrets = Vec::new();
        if contracts {
            let binary = process::executable(&std::env::var("ROSTFREI_NATS_SERVER").unwrap_or("nats-server".into()))
                .context("contract tests require native NATS: set ROSTFREI_NATS_SERVER to a cached nats-server binary")?;
            let password = format!("{}{}", nonce(), nonce());
            config.push_str(&format!(
                "\nauthorization {{ user: \"contract-tests\", password: \"{password}\" }}\n"
            ));
            env.insert(
                "ROSTFREI_CONTRACT_NATS_USER".into(),
                "contract-tests".into(),
            );
            env.insert("ROSTFREI_CONTRACT_NATS_PASSWORD".into(), password.clone());
            env.insert(
                "ROSTFREI_CONTRACT_NATS_BINARY".into(),
                binary.display().to_string(),
            );
            env.insert("ROSTFREI_NATS_SERVER".into(), binary.display().to_string());
            env.insert(
                "ROSTFREI_CONTRACT_EVIDENCE_DIR".into(),
                private.path().display().to_string(),
            );
            secrets.push(password);
        }
        let config_path = private.path().join("nats.conf");
        private_write(&config_path, config.as_bytes())?;
        docker(&["info"], &secrets)?;
        let mut broker = Self {
            name: format!("rostfrei-test-nats-{}", nonce()),
            armed: true,
            env,
            secrets,
            _private: private,
        };
        let result = (|| {
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
                    &format!(
                        "type=bind,src={},dst=/etc/nats/test.conf,readonly",
                        config_path.display()
                    ),
                    IMAGE,
                    "--config",
                    "/etc/nats/test.conf",
                ],
                &broker.secrets,
            )?;
            let ports: Value = serde_json::from_str(&docker(
                &[
                    "inspect",
                    "--format",
                    "{{json .NetworkSettings.Ports}}",
                    &broker.name,
                ],
                &broker.secrets,
            )?)?;
            let port = mapped_port(&ports, "4222/tcp")?;
            wait_ready(mapped_port(&ports, "8222/tcp")?, ready_timeout)?;
            let auth = broker
                .secrets
                .first()
                .map_or(String::new(), |p| format!("contract-tests:{p}@"));
            broker.env.insert(
                "ROSTFREI_NATS_URL".into(),
                format!("nats://{auth}127.0.0.1:{port}"),
            );
            println!("NATS 2.12.15 ready on isolated port {port}");
            Ok::<_, anyhow::Error>(())
        })();
        if let Err(error) = result {
            broker.logs();
            return Err(error);
        }
        Ok(broker)
    }
    pub fn logs(&self) {
        if let Ok(logs) = docker(&["logs", &self.name], &self.secrets) {
            eprintln!("{logs}");
        }
    }
    pub fn shutdown(mut self) -> Result<()> {
        // Cleanup must run even after cancellation was observed by the child runner.
        cleanup(&self.name)?;
        self.armed = false;
        Ok(())
    }
}
fn cleanup(name: &str) -> Result<()> {
    let mut child = std::process::Command::new("docker")
        .args(["rm", "--force", "--volumes", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(status.success(), "Could not remove test container {name}");
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Could not remove test container {name}: timeout");
        }
        thread::sleep(Duration::from_millis(20));
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        if self.armed
            && let Err(error) = cleanup(&self.name)
        {
            eprintln!("{error}");
        }
    }
}
pub fn run(command: Vec<String>, contracts: bool, timeout: Duration) -> Result<i32> {
    let default = command.is_empty();
    if default {
        let prepared = process::capture(
            &process::args(&[
                "cargo",
                "fetch",
                "--locked",
                "--manifest-path",
                &root().join(versions::FIXTURE).display().to_string(),
            ]),
            &root(),
            &environment(),
            Duration::from_secs(1200),
            &[],
            true,
        )?;
        if prepared.code != 0 {
            return Ok(prepared.code);
        }
    }
    let command = if default {
        process::args(DEFAULT)
    } else {
        command
    };
    let broker = Broker::start(contracts || command == process::args(DEFAULT), timeout)?;
    let result = process::capture(
        &command,
        &root(),
        &broker.env,
        Duration::from_secs(7200),
        &broker.secrets,
        true,
    );
    if !result.as_ref().is_ok_and(|r| r.code == 0) {
        broker.logs();
    }
    broker.shutdown()?;
    Ok(result?.code)
}
pub fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
pub fn native(binary: PathBuf, evidence: PathBuf, command: Vec<String>) -> Result<i32> {
    ensure!(!command.is_empty(), "supply the exact command after --");
    let binary = binary.canonicalize()?;
    let run = crate::absolute(&evidence)?.join(format!("run-{}", nonce()));
    fs::create_dir_all(&run)?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&run, fs::Permissions::from_mode(0o700))?;
    let port = port()?;
    let monitor = self::port()?;
    let password = nonce();
    let config = run.join("nats.conf");
    private_write(&config, format!("host: \"127.0.0.1\"\nport: {port}\nhttp: \"127.0.0.1:{monitor}\"\nmax_payload: 2113536\nauthorization {{ user: \"contract-tests\", password: \"{password}\" }}\njetstream {{ store_dir: {:?} }}\n", run.join("jetstream")).as_bytes())?;
    let mut env = environment();
    limits(&mut env);
    let toolchain = versions::read(&root().join("rust-toolchain.toml"))?["toolchain"]["channel"]
        .as_str()
        .context("pinned toolchain channel")?
        .to_owned();
    let rustc = process::run(
        &["rustup", "which", "--toolchain", &toolchain, "rustc"],
        &root(),
        &env,
    )
    .context("pinned cached toolchain missing; this command does not install it")?
    .trim()
    .to_owned();
    let rustdoc = process::run(
        &["rustup", "which", "--toolchain", &toolchain, "rustdoc"],
        &root(),
        &env,
    )?
    .trim()
    .to_owned();
    let bin = Path::new(&rustc)
        .parent()
        .context("toolchain bin directory")?;
    let mut paths = vec![bin.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    env.insert(
        "PATH".into(),
        std::env::join_paths(paths)?.to_string_lossy().into_owned(),
    );
    env.insert("RUSTUP_TOOLCHAIN".into(), toolchain.clone());
    env.insert("RUSTC".into(), rustc.clone());
    env.insert("RUSTDOC".into(), rustdoc);
    env.insert("CARGO_BUILD_JOBS".into(), "2".into());
    env.insert(
        "CARGO_TARGET_DIR".into(),
        root()
            .join(format!("target/release-contract-{toolchain}"))
            .display()
            .to_string(),
    );
    for (key, value) in [
        (
            "ROSTFREI_NATS_URL",
            format!("nats://contract-tests:{password}@127.0.0.1:{port}"),
        ),
        ("ROSTFREI_CONTRACT_NATS_USER", "contract-tests".into()),
        ("ROSTFREI_CONTRACT_NATS_PASSWORD", password.clone()),
        (
            "ROSTFREI_CONTRACT_NATS_BINARY",
            binary.display().to_string(),
        ),
        ("ROSTFREI_NATS_SERVER", binary.display().to_string()),
        ("ROSTFREI_CONTRACT_EVIDENCE_DIR", run.display().to_string()),
    ] {
        env.insert(key.into(), value);
    }
    let mut manifest = json!({"root": root(), "command": command, "head": process::run(&["git", "rev-parse", "HEAD"], &root(), &environment())?.trim(),
        "nats_version": process::run(&[&binary.display().to_string(), "--version"], &root(), &environment())?.trim(), "broker_config": config});
    manifest["rust_version"] = json!(process::run(&[&rustc, "--version"], &root(), &env)?.trim());
    manifest["environment"] = json!(
        env.iter()
            .filter(|(key, _)| key.starts_with("ROSTFREI_")
                || [
                    "RUSTUP_TOOLCHAIN",
                    "RUSTC",
                    "RUSTDOC",
                    "CARGO_BUILD_JOBS",
                    "CARGO_TARGET_DIR",
                    "RUST_BACKTRACE"
                ]
                .contains(&key.as_str()))
            .map(|(key, value)| (
                key.clone(),
                process::redact(value.clone(), std::slice::from_ref(&password))
            ))
            .collect::<Environment>()
    );
    let mut broker = process::background(
        &[
            binary.display().to_string(),
            "--config".into(),
            config.display().to_string(),
        ],
        &root(),
        &env,
        &run.join("broker.log"),
    )?;
    manifest["owned_broker_pid"] = json!(broker.child.id());
    println!("Evidence: {}", run.display());
    let result = (|| {
        wait_ready(monitor, Duration::from_secs(15))?;
        manifest["broker_ready"] = json!(true);
        let result = process::capture_logged(
            &command,
            &root(),
            &env,
            Duration::from_secs(1200),
            &[password],
            true,
            Some(&run.join("cargo.log")),
        )?;
        manifest["cargo_exit_code"] = json!(result.code);
        for (route, name) in [
            ("varz", "broker-varz.json"),
            (
                "jsz?streams=true&consumers=true&config=true",
                "broker-jsz.json",
            ),
        ] {
            fs::write(
                run.join(name),
                client(Duration::from_secs(3))?
                    .get(format!("http://127.0.0.1:{monitor}/{route}"))
                    .send()?
                    .error_for_status()?
                    .bytes()?,
            )?;
        }
        Ok::<_, anyhow::Error>(result.code)
    })();
    if let Err(error) = &result {
        manifest["runner_error"] = json!(error.to_string());
    }
    let shutdown = broker.stop(Duration::from_secs(10));
    manifest["owned_broker_exit_code"] = json!(shutdown.as_ref().ok());
    save_json(&run.join("manifest.json"), &manifest)?;
    shutdown?;
    result
}
