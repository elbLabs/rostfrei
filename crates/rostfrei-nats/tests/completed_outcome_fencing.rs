//! Real authenticated broker policy: old writers/rollback cannot mutate the new epoch.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use std::{
    ffi::OsStr,
    path::PathBuf,
    process::{Child, Command},
};
rostfrei_runtime::install_macro_support!();

struct Broker {
    child: Child,
    directory: PathBuf,
}
impl Drop for Broker {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
        let status = self.child.wait();
        let _ = std::fs::write(
            self.directory.join("owned-broker-cleanup.txt"),
            format!("owned pid={} wait={status:?}", self.child.id()),
        );
    }
}

async fn broker(label: &str) -> TestResult<(Broker, String)> {
    start_broker(label, "ROSTFREI_CONTRACT_NATS_BINARY").await
}

async fn recovery_broker(label: &str) -> TestResult<(Broker, String)> {
    start_broker(label, "ROSTFREI_RECOVERY_NATS_SERVER").await
}

async fn start_broker(label: &str, binary_var: &str) -> TestResult<(Broker, String)> {
    let directory = PathBuf::from(std::env::var("ROSTFREI_CONTRACT_EVIDENCE_DIR")?)
        .join(format!("outcome-{label}-{}", std::process::id()));
    std::fs::create_dir(&directory)?;
    let socket = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let config = directory.join("nats.conf");
    // Nominal 128-GiB reservation budget: the default split pair needs 20 GiB,
    // regardless of builder free disk. Actual records remain fixture-sized.
    std::fs::write(
        &config,
        format!(
            r#"host: "127.0.0.1"
port: {port}
max_payload: 2113536
jetstream {{ store_dir: "{}", max_file_store: 137438953472 }}
authorization {{ users: [
 {{ user: "new-writer", password: "private-new-writer" }},
 {{ user: "old-writer", password: "private-old-writer", permissions: {{
    publish: ["$JS.API.STREAM.INFO.>", "$JS.API.STREAM.MSG.GET.>", "$JS.API.CONSUMER.>"],
    subscribe: ["_INBOX.>"]
 }} }}
] }}
"#,
            directory.join("jetstream").display()
        ),
    )?;
    let url = format!("nats://127.0.0.1:{port}");
    let broker = open_broker(
        directory,
        &url,
        &std::env::var_os(binary_var).ok_or_else(|| format!("missing {binary_var}"))?,
    )
    .await?;
    Ok((broker, url))
}

async fn open_broker(directory: PathBuf, url: &str, binary: &OsStr) -> TestResult<Broker> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("broker.log"))?;
    let child = Command::new(binary)
        .arg("--config")
        .arg(directory.join("nats.conf"))
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    let broker = Broker { child, directory };
    tokio::time::timeout(WAIT, async {
        loop {
            if async_nats::ConnectOptions::with_user_and_password(
                "new-writer".to_owned(),
                "private-new-writer".to_owned(),
            )
            .connect(url)
            .await
            .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    Ok(broker)
}

async fn restart_recovery(broker: Broker, url: &str) -> TestResult<Broker> {
    let directory = broker.directory.clone();
    drop(broker);
    open_broker(
        directory,
        url,
        &std::env::var_os("ROSTFREI_RECOVERY_NATS_SERVER").ok_or("missing recovery server")?,
    )
    .await
}

async fn assert_raw_writer_refused(
    client: &async_nats::Client,
    stream: &mut async_nats::jetstream::stream::Stream,
    errors: &tokio::sync::Mutex<Vec<String>>,
    subjects: Vec<String>,
) -> TestResult {
    let before = stream.info().await?.state.messages;
    for subject in subjects {
        let before_errors = errors.lock().await.len();
        let result = client
            .send_request(
                subject,
                async_nats::Request::new()
                    .payload(b"old mutable writer".as_slice().into())
                    .timeout(Some(Duration::from_millis(150))),
            )
            .await;
        assert!(
            result.is_err(),
            "old writer must receive no successful publish/API acknowledgement"
        );
        tokio::time::timeout(WAIT, async {
            loop {
                if errors.lock().await.len() > before_errors {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        assert_eq!(stream.info().await?.state.messages, before);
    }
    Ok(())
}

async fn old_reader(
    url: &str,
    collected: Arc<tokio::sync::Mutex<Vec<String>>>,
) -> TestResult<async_nats::Client> {
    Ok(async_nats::ConnectOptions::with_user_and_password(
        "old-writer".to_owned(),
        "private-old-writer".to_owned(),
    )
    .event_callback(move |event| {
        let collected = collected.clone();
        async move {
            if let async_nats::Event::ServerError(error) = event {
                collected.lock().await.push(format!("{error:?}"));
            }
        }
    })
    .connect(url)
    .await?)
}

#[tokio::test]
async fn native_acl_excludes_old_mutable_writer_and_rollback_but_allows_read_only_replay()
-> TestResult {
    let (broker, url) = broker("acl").await?;
    let new_client = async_nats::ConnectOptions::with_user_and_password(
        "new-writer".to_owned(),
        "private-new-writer".to_owned(),
    )
    .connect(&url)
    .await?;
    let js = async_nats::jetstream::new(new_client.clone());
    let context =
        ApplicationName::new("fenced-outcome-epoch")?.bounded_context("release-contract")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_split_outcome_epoch("epoch92")?;
    provision_event_store(&js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(js.clone(), config.clone()).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&context, store.clone(), calls.clone(), None)?;
    let local = local_bus(&context, processor.clone());
    let command = local.encode(request("fenced-rejection", "missing", "add")?)?;
    let rejected = processor.process(&command).await?;
    assert!(release_contract_support::rejected(rejected.outcome()));
    let mut stream_info = js.get_stream(config.stream_name()).await?;
    let errors = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let old_client = old_reader(&url, errors.clone()).await?;
    let old_js = async_nats::jetstream::new(old_client.clone());
    // Native ACL limits old credentials to reads. Even raw old-protocol publication is
    // refused; this does not rely on an old binary understanding new ledger metadata.
    assert_raw_writer_refused(
        &old_client,
        &mut stream_info,
        &errors,
        vec![
            config.event_subject("target", "missing"),
            config.transaction_receipt_subject("rollback-acceptance", None, None),
            command.address().as_str().to_owned(),
            command.response_address()?.as_str().to_owned(),
            format!("$JS.API.STREAM.UPDATE.{}", config.stream_name()),
        ],
    )
    .await?;
    std::fs::write(
        broker.directory.join("native-acl-errors.json"),
        serde_json::to_vec_pretty(&*errors.lock().await)?,
    )?;
    let readonly_config = config.clone().with_read_only_outcomes();
    let readonly = Arc::new(NatsEventStore::connect(old_js, readonly_config).await?);
    let readonly_processor = processor_for(&context, readonly.clone(), calls.clone())?;
    assert_eq!(readonly_processor.process(&command).await?, rejected);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "read-only rollback reader cannot reevaluate"
    );
    let unknown = local.encode(request("new-unknown-operation", "missing", "add")?)?;
    assert!(readonly_processor.process(&unknown).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let transaction = EventTransaction::new(
        OperationId::new("read-only-write")?,
        ContentFingerprint::digest("read-only"),
        Vec::new(),
    )
    .with_bounded_context(context.name().clone());
    assert_eq!(
        readonly
            .append_transaction(transaction)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    let wrong_epoch = config.with_fresh_completed_outcome_epoch("epoch93")?;
    assert_eq!(
        NatsEventStore::connect(js.clone(), wrong_epoch.clone())
            .await
            .err()
            .ok_or("wrong epoch connected")?
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    assert_eq!(
        update_event_store(&js, &wrong_epoch)
            .await
            .unwrap_err()
            .kind(),
        EventStoreErrorKind::ConfigurationMismatch
    );
    old_client.drain().await?;
    new_client.drain().await?;
    drop(broker);
    Ok(())
}

fn processor_for(
    context: &BoundedContext,
    store: Arc<NatsEventStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<CommandProcessor>> {
    processor(context, store, calls, None)
}

fn paired_snapshot(
    mode: &str,
    url: &str,
    config: &NatsEventStoreConfig,
    path: &std::path::Path,
) -> TestResult {
    let empty_path = tempfile::tempdir()?;
    let output = Command::new(
        std::env::var_os("ROSTFREI_STORE_CLI")
            .ok_or("build cargo-rostfrei and set ROSTFREI_STORE_CLI to its absolute path")?,
    )
    .env_clear()
    .env("PATH", empty_path.path())
    .args(["rostfrei", "store", mode])
    .arg(path)
    .args([config.stream_name(), config.outcome_state_stream_name()])
    .env("NATS_URL", url)
    .env("NATS_USER", "new-writer")
    .env("NATS_PASSWORD", "private-new-writer")
    .output()?;
    if !output.status.success() {
        return Err(format!("{mode}: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(())
}

async fn writer_context(url: &str) -> TestResult<async_nats::jetstream::Context> {
    Ok(async_nats::jetstream::new(
        async_nats::ConnectOptions::with_user_and_password(
            "new-writer".to_owned(),
            "private-new-writer".to_owned(),
        )
        .connect(url)
        .await?,
    ))
}

// A real event large enough that even its compressed snapshot needs chunk ACKs.
fn large_item() -> TestResult<CommandRequest<Act>> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut item = String::new();
    for i in 0..4096_u32 {
        write!(item, "{:x}", Sha256::digest(i.to_be_bytes()))?;
    }
    Ok(CommandRequest::new(
        OperationId::new("large-item")?,
        Act {
            action: "add".to_owned(),
            target: "target".to_owned(),
            item,
        },
    ))
}

#[tokio::test]
#[ignore = "paired restore qualification requires the Rostfrei CLI and private server fixtures"]
async fn paired_restore_replays_original_decisions_before_resuming_writes() -> TestResult {
    let (source, url) = recovery_broker("backup").await?;
    let context = ApplicationName::new("paired-restore")?.bounded_context("release-contract")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_outcomes()?
        .with_explicit_detail_expiry(true)?
        .with_storage_limits(32 * 1024 * 1024, 524_288)?;
    let js = writer_context(&url).await?;
    provision_event_store(&js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(js.clone(), config.clone()).await?);
    let authority = store.authority().clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let seed = processor(&context, store.clone(), calls.clone(), None)?;
    let bus = local_bus(&context, seed.clone());
    let add = bus.encode(request("before-create", "target", "add")?)?;
    let create = bus.encode(request("create-target", "target", "create")?)?;
    let refusal = seed.process(&add).await?;
    let acceptance = seed.process(&create).await?;
    assert!(rejected(refusal.outcome()));
    assert_eq!(acceptance.outcome(), &CommandResponseOutcome::Accepted);
    let large = bus.encode(large_item()?)?;
    assert_eq!(
        seed.process(&large).await?.outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let key = OutcomeKey::new(
        authority.clone(),
        context.name().clone(),
        OperationId::new(large.operation_id().as_str())?,
    );
    let OutcomeLookup::Completed(original) = store.lookup_outcome(&key).await? else {
        return Err("missing outcome to expire".into());
    };
    store
        .expire_details(&key, original.marker().revision())
        .await?;
    let id = stream("target")?;
    let history = store.load(&id).await?;
    assert_eq!(history.len(), 2);
    drop((bus, seed, store, js));
    let source = restart_recovery(source, &url).await?;

    let backup = tempfile::tempdir()?;
    let path = backup.path().join("pair");
    paired_snapshot("backup", &url, &config, &path)?;
    assert!(
        path.join("journal.tar.s2").metadata()?.len() > 128 * 1024,
        "multi-chunk journal fixture became too small"
    );
    drop(source);
    let (destination, url) = recovery_broker("restore").await?;
    paired_snapshot("restore", &url, &config, &path)?;
    let _restarted = restart_recovery(destination, &url).await?;
    let js = writer_context(&url).await?;
    let reader = Arc::new(
        NatsEventStore::connect(js.clone(), config.clone().with_read_only_outcomes()).await?,
    );
    assert_eq!(reader.authority(), &authority);
    let verify = processor(&context, reader.clone(), calls.clone(), None)?;
    assert_eq!(verify.process(&add).await?, refusal);
    assert_eq!(verify.process(&create).await?, acceptance);
    assert_eq!(
        verify.process(&large).await.unwrap_err().kind(),
        rostfrei_runtime::CommandProcessorErrorKind::CompletedDetailsExpired
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        3,
        "exact retries ran a handler"
    );
    assert_eq!(reader.load(&id).await?, history);
    let restored = Arc::new(NatsEventStore::connect(js, config).await?);
    let fresh = processor(&context, restored.clone(), calls.clone(), None)?;
    let new_command =
        local_bus(&context, fresh.clone()).encode(request("after-restore", "target", "add")?)?;
    assert_eq!(
        fresh.process(&new_command).await?.outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(restored.load(&id).await?.len(), 3);
    Ok(())
}

#[tokio::test]
#[ignore = "uses the native 2.12.15 framework fixture and Rostfrei CLI"]
async fn legacy_broker_is_refused_before_creating_a_backup() -> TestResult {
    let (_source, url) = broker("unsupported-recovery").await?;
    let context = ApplicationName::new("legacy-backup")?.bounded_context("release-contract")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?.with_fresh_outcomes()?;
    let backup = tempfile::tempdir()?;
    let path = backup.path().join("not-created");
    let error = paired_snapshot("backup", &url, &config, &path).unwrap_err();
    assert!(
        error.to_string().contains("requires NATS Server 2.14.10"),
        "{error}"
    );
    assert!(!path.exists());
    Ok(())
}

#[tokio::test]
#[ignore = "paired restore qualification requires the Rostfrei CLI and private server fixtures"]
async fn partial_destination_is_refused_before_restoring_either_stream() -> TestResult {
    let (_broker, url) = recovery_broker("partial").await?;
    let context = ApplicationName::new("partial-restore")?.bounded_context("release-contract")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?.with_fresh_outcomes()?;
    let js = writer_context(&url).await?;
    provision_event_store(&js, &config).await?;
    let backup = tempfile::tempdir()?;
    let path = backup.path().join("pair");
    paired_snapshot("backup", &url, &config, &path)?;

    js.delete_stream(config.stream_name()).await?;
    let error = paired_snapshot("restore", &url, &config, &path).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("both destination streams are absent"),
        "{error}"
    );
    let missing = js.get_stream(config.stream_name()).await.unwrap_err();
    assert!(
        matches!(missing.kind(), async_nats::jetstream::context::GetStreamErrorKind::JetStream(ref err) if err.code() == 404)
    );
    Ok(())
}

#[tokio::test]
#[ignore = "paired restore qualification requires the Rostfrei CLI and private server fixtures"]
async fn damaged_results_archive_is_refused_before_restoring_journal() -> TestResult {
    let (_broker, url) = recovery_broker("damaged").await?;
    let context = ApplicationName::new("damaged-backup")?.bounded_context("release-contract")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?.with_fresh_outcomes()?;
    let js = writer_context(&url).await?;
    provision_event_store(&js, &config).await?;
    let backup = tempfile::tempdir()?;
    let path = backup.path().join("pair");
    paired_snapshot("backup", &url, &config, &path)?;
    js.delete_stream(config.stream_name()).await?;
    js.delete_stream(config.outcome_state_stream_name()).await?;
    std::fs::write(path.join("results.tar.s2"), b"truncated")?;
    let error = paired_snapshot("restore", &url, &config, &path).unwrap_err();
    assert!(error.to_string().contains("archive checksum"), "{error}");
    for name in [config.stream_name(), config.outcome_state_stream_name()] {
        let missing = js.get_stream(name).await.unwrap_err();
        assert!(
            matches!(missing.kind(), async_nats::jetstream::context::GetStreamErrorKind::JetStream(ref err) if err.code() == 404)
        );
    }
    Ok(())
}
