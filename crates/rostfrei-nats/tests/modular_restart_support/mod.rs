//! Owned authenticated crash/restart fixture. Does not touch the runner's broker.
use std::{
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::Duration,
};
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Server {
    child: Child,
    directory: PathBuf,
}
impl Server {
    fn spawn(directory: &Path, phase: &str) -> TestResult<Self> {
        let log = File::create(directory.join(format!("broker-{phase}.log")))?;
        let child = Command::new(std::env::var("ROSTFREI_CONTRACT_NATS_BINARY")?)
            .arg("--config")
            .arg(directory.join("nats.conf"))
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        Ok(Self {
            child,
            directory: directory.to_owned(),
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let status = self.child.wait();
        let _ = std::fs::write(
            self.directory
                .join(format!("cleanup-{}.txt", self.child.id())),
            format!("owned pid={} wait={status:?}\n", self.child.id()),
        );
    }
}
async fn ready(url: &str) -> TestResult {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(client) = async_nats::ConnectOptions::with_user_and_password(
                "modular-restart".to_owned(),
                "owned-fixture".to_owned(),
            )
            .connect(url)
            .await
                && async_nats::jetstream::new(client)
                    .query_account()
                    .await
                    .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    Ok(())
}
fn child(directory: &Path, test: &str, url: &str, phase: &str) -> TestResult {
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", test, "--nocapture"])
        .env("ROSTFREI_MODULAR_RESTART_MODE", phase)
        .env("ROSTFREI_MODULAR_RESTART_RECORDS", directory)
        .env(
            "ROSTFREI_MODULAR_RESTART_APP",
            format!("modular-restart-{}", std::process::id()),
        )
        .env("ROSTFREI_NATS_URL", url)
        .env("ROSTFREI_CONTRACT_NATS_USER", "modular-restart")
        .env("ROSTFREI_CONTRACT_NATS_PASSWORD", "owned-fixture")
        .output()?;
    let mut bytes = output.stdout;
    bytes.extend_from_slice(&output.stderr);
    std::fs::write(directory.join(format!("child-{phase}.log")), &bytes)?;
    std::fs::write(
        directory.join(format!("child-{phase}-exit.txt")),
        output.status.to_string(),
    )?;
    if !output.status.success() {
        return Err(format!(
            "child {phase}: {}: {}",
            output.status,
            String::from_utf8_lossy(&bytes)
        )
        .into());
    }
    Ok(())
}
pub async fn run(test: &str) -> TestResult {
    let directory = PathBuf::from(std::env::var("ROSTFREI_CONTRACT_EVIDENCE_DIR")?)
        .join(format!("modular-restart-{}", std::process::id()));
    std::fs::create_dir(&directory)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // Keep owned restart reservation accounting independent of builder free disk;
    // 128 GiB is nominal capacity, not allocated data or a stream-policy change.
    std::fs::write(
        directory.join("nats.conf"),
        format!(
            "host: \"127.0.0.1\"\nport: {port}\nmax_payload: 2113536\nauthorization {{ user: \"modular-restart\", password: \"owned-fixture\" }}\njetstream {{ store_dir: \"{}\", max_file_store: 137438953472 }}\n",
            directory.join("jetstream").display()
        ),
    )?;
    drop(listener);
    let url = format!("nats://127.0.0.1:{port}");
    let server = Server::spawn(&directory, "seed")?;
    ready(&url).await?;
    child(&directory, test, &url, "seed")?;
    drop(server); // hard crash and wait; private file store intentionally retained
    let _restarted = Server::spawn(&directory, "replay")?;
    ready(&url).await?;
    child(&directory, test, &url, "replay")?;
    Ok(())
}
