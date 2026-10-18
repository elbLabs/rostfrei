//! #92: no ignored subprocess entry point, in-memory cache, Docker, or live-broker restart.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use std::{
    fs::File,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command},
};

const TEST: &str = "completed_rejection_and_acceptance_survive_process_and_broker_restart";
const MODE: &str = "ROSTFREI_CONTRACT_RESTART_MODE";
const APP: &str = "ROSTFREI_CONTRACT_RESTART_APPLICATION";

struct Server {
    child: Child,
    binary: PathBuf,
    config: PathBuf,
    directory: PathBuf,
}
impl Server {
    fn spawn(
        binary: PathBuf,
        config: PathBuf,
        directory: PathBuf,
        label: &str,
    ) -> TestResult<Self> {
        let log = File::create(directory.join(format!("broker-{label}.log")))?;
        let child = Command::new(&binary)
            .args(["--config"])
            .arg(&config)
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        Ok(Self {
            child,
            binary,
            config,
            directory,
        })
    }
    fn restart(mut self) -> TestResult<Self> {
        // Crash only this owned child. Preserve its complete private file store.
        self.child.kill()?;
        let status = self.child.wait()?;
        std::fs::write(
            self.directory.join("restart-exit.txt"),
            format!("owned broker pid={} status={status}\n", self.child.id()),
        )?;
        Self::spawn(
            self.binary.clone(),
            self.config.clone(),
            self.directory.clone(),
            "restarted",
        )
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
                .join(format!("owned-broker-cleanup-{}.txt", self.child.id())),
            format!("owned pid={} wait={status:?}\n", self.child.id()),
        );
    }
}
async fn ready(url: &str) -> TestResult {
    tokio::time::timeout(WAIT, async {
        loop {
            if let Ok(client) = async_nats::ConnectOptions::with_user_and_password(
                "restart-user".to_owned(),
                "restart-pass".to_owned(),
            )
            .connect(url)
            .await
                && async_nats::jetstream::new(client)
                    .query_account()
                    .await
                    .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    Ok(())
}
fn child(directory: &std::path::Path, url: &str, application: &str, mode: &str) -> TestResult {
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", TEST, "--nocapture"])
        .env(MODE, mode)
        .env(APP, application)
        .env("ROSTFREI_NATS_URL", url)
        .env("ROSTFREI_CONTRACT_NATS_USER", "restart-user")
        .env("ROSTFREI_CONTRACT_NATS_PASSWORD", "restart-pass")
        .output()?;
    let mut log = output.stdout;
    log.extend_from_slice(&output.stderr);
    std::fs::write(directory.join(format!("child-{mode}.log")), &log)?;
    std::fs::write(
        directory.join(format!("child-{mode}-exit.txt")),
        format!("{}\n", output.status),
    )?;
    assert!(
        output.status.success(),
        "independent {mode} process failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&log)
    );
    Ok(())
}
async fn phase(mode: &str) -> TestResult {
    let fixture = Fixture::for_application(ApplicationName::new(std::env::var(APP)?)?).await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&fixture.context, processor.clone());
    let rejected_request = request("durable-rejection", "target", "add")?;
    let no_event = request("durable-noevent", "unused", "noop")?;
    if mode == "seed" {
        assert!(rejected(
            processor
                .process(&local.encode(rejected_request)?)
                .await?
                .outcome()
        ));
        local
            .dispatch(request("repair-prerequisite", "target", "create")?)
            .await?;
        assert_eq!(
            local.dispatch(no_event).await?.response().outcome(),
            &CommandResponseOutcome::Accepted
        );
        return Ok(());
    }
    assert_eq!(mode, "replay");
    // No-event acceptance is an independently durable existing control.
    assert_eq!(
        processor
            .process(&local.encode(no_event.clone())?)
            .await?
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        rejected(
            processor
                .process(&local.encode(rejected_request.clone())?)
                .await?
                .outcome()
        ),
        "completed rejection must survive a different application process and broker restart"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _worker = fixture.command_worker(processor).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    assert!(rejected(
        remote
            .dispatch(rejected_request)
            .await?
            .response()
            .outcome()
    ));
    assert_eq!(
        remote.dispatch(no_event).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    assert!(store.load(&stream("unused")?).await?.is_empty());
    Ok(())
}
#[tokio::test]
async fn completed_rejection_and_acceptance_survive_process_and_broker_restart() -> TestResult {
    if let Ok(mode) = std::env::var(MODE) {
        return phase(&mode).await;
    }
    let directory = PathBuf::from(std::env::var("ROSTFREI_CONTRACT_EVIDENCE_DIR")?)
        .join(format!("restart-{}", std::process::id()));
    std::fs::create_dir(&directory)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let config = directory.join("nats.conf");
    std::fs::write(
        &config,
        format!(
            "host: \"127.0.0.1\"\nport: {port}\nmax_payload: 2113536\nauthorization {{ user: \"restart-user\", password: \"restart-pass\" }}\njetstream {{ store_dir: \"{}\" }}\n",
            directory.join("jetstream").display()
        ),
    )?;
    drop(listener);
    let binary = PathBuf::from(std::env::var("ROSTFREI_CONTRACT_NATS_BINARY")?);
    let server = Server::spawn(binary, config, directory.clone(), "seed")?;
    let url = format!("nats://127.0.0.1:{port}");
    ready(&url).await?;
    let application = format!("contract-restart-{}", std::process::id());
    child(&directory, &url, &application, "seed")?;
    let _server = server.restart()?;
    ready(&url).await?;
    child(&directory, &url, &application, "replay")?;
    Ok(())
}
rostfrei::install_macro_support!();
