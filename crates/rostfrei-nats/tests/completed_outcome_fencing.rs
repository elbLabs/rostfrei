//! Real authenticated broker policy: old writers/rollback cannot mutate the new epoch.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use std::{
    fs::File,
    path::PathBuf,
    process::{Child, Command},
};
rostfrei::install_macro_support!();

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

async fn broker() -> TestResult<(Broker, String)> {
    let directory = PathBuf::from(std::env::var("ROSTFREI_CONTRACT_EVIDENCE_DIR")?)
        .join(format!("outcome-acl-{}", std::process::id()));
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
    let log = File::create(directory.join("broker.log"))?;
    let child = Command::new(std::env::var("ROSTFREI_CONTRACT_NATS_BINARY")?)
        .arg("--config")
        .arg(config)
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    let broker = Broker { child, directory };
    let url = format!("nats://127.0.0.1:{port}");
    tokio::time::timeout(WAIT, async {
        loop {
            if async_nats::ConnectOptions::with_user_and_password(
                "new-writer".to_owned(),
                "private-new-writer".to_owned(),
            )
            .connect(&url)
            .await
            .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?;
    Ok((broker, url))
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
    let (broker, url) = broker().await?;
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
            config.aggregate_subject("target", "missing"),
            config.transaction_subject("rollback-acceptance"),
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
