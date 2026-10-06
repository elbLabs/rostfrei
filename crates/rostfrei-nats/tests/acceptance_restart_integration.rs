//! Durable acceptance across application processes, broker restart and deduplication expiry.
//! Self-contained Docker fixture, like `authentication_integration`.

use std::{
    convert::Infallible,
    process::Command as Process,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use rostfrei::{
    Command, CommandBus, CommandProcessor, CommandRequest, InMemoryMessagingAdapter,
    InfallibleCommandRejectionMapper,
};
use rostfrei_core::{
    Aggregate, AggregateId, AggregateType, CommandDecision, CommandExecution,
    CommandExecutionMetadata, CommandExecutor, CommandHandler, CommandHandlingResult,
    CommandOutcome, CommandReceipt, ContentFingerprint, Event, EventCodecError, EventStore,
    EventTransaction, OperationId, RecordedEvent, StreamDirectory, StreamId,
};
use rostfrei_messaging_core::{
    ApplicationName, BoundedContext, CallerMetadata, CausationId, CommandAddress,
    CommandResponseOutcome, CorrelationId, DeliveryDisposition, DeliveryInfo, MessageDelivery,
    MessageHandler,
};
use rostfrei_nats::{
    ApplicationMessagingConfig, NatsCommandResponseReader, NatsEventStore, NatsEventStoreConfig,
    NatsMessagingAdapter, NatsPublisher, provision_application_messaging, provision_event_store,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const WAIT: Duration = Duration::from_secs(15);
const URL_ENV: &str = "ROSTFREI_ACCEPTANCE_RESTART_URL";

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[derive(rostfrei::BoundedContext)]
#[domain(id = "acceptance", label = "Acceptance")]
struct Acceptance;

#[derive(Command)]
#[domain(context = Acceptance, id = "accept", label = "Accept")]
struct Accept {
    content: String,
    guarded: bool,
}

struct Guard;
struct UnusedEvent;

impl Aggregate for Guard {
    type State = ();
    type Event = UnusedEvent;
    const BOUNDED_CONTEXT: &'static str = "acceptance";
    const AGGREGATE_TYPE: &'static str = "guard";
    fn initial(_stream_id: &StreamId) {}
    fn apply(_state: &mut (), _event: &UnusedEvent) {}
}

impl Event for UnusedEvent {
    fn event_type(&self) -> &'static str {
        "unused"
    }
    fn schema_version(&self) -> u32 {
        1
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        Ok(b"{}".to_vec())
    }
    fn decode_json(_event: &RecordedEvent) -> Result<Self, EventCodecError> {
        Ok(Self)
    }
}

struct AcceptHandler(Arc<AtomicUsize>);

#[async_trait]
impl CommandHandler<Accept> for AcceptHandler {
    type Rejection = Infallible;
    async fn handle(
        &self,
        command: &Accept,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Infallible> {
        self.0.fetch_add(1, Ordering::Relaxed);
        if command.guarded {
            let _guard = execution.load::<Guard>("observed").await?;
        }
        Ok(CommandDecision::Accepted)
    }
}

fn context() -> TestResult<BoundedContext> {
    Ok(ApplicationName::new("acceptance-restart")?.bounded_context("acceptance")?)
}

fn request(operation: &str, content: &str, guarded: bool) -> TestResult<CommandRequest<Accept>> {
    Ok(CommandRequest::new(
        OperationId::new(operation)?,
        Accept {
            content: content.to_owned(),
            guarded,
        },
    )
    .with_correlation_id(CorrelationId::new("correlation")?)
    .with_causation_id(CausationId::new("cause")?))
}

fn processor(
    store: Arc<dyn EventStore>,
    calls: Arc<AtomicUsize>,
) -> TestResult<Arc<CommandProcessor>> {
    let mut processor = CommandProcessor::new(context()?.name().clone(), store);
    processor.register::<Accept, _>(AcceptHandler(calls), InfallibleCommandRejectionMapper)?;
    Ok(Arc::new(processor))
}

fn docker(args: &[&str]) -> TestResult<String> {
    let output = Process::new("docker").args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

struct Server(String);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = docker(&["rm", "--force", "--volumes", &self.0]);
    }
}

async fn connect_ready(url: &str) -> TestResult<async_nats::jetstream::Context> {
    Ok(tokio::time::timeout(WAIT, async {
        loop {
            if let Ok(client) = async_nats::connect(url).await {
                let context = async_nats::jetstream::new(client);
                if context.query_account().await.is_ok() {
                    return context;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?)
}

fn child(url: &str, mode: &str) -> TestResult {
    let output = Process::new(std::env::current_exe()?)
        .args(["--ignored", "--exact", "acceptance_child", "--nocapture"])
        .env(URL_ENV, url)
        .env("ROSTFREI_ACCEPTANCE_RESTART_MODE", mode)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "acceptance child {mode} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

async fn dedup_probe(
    js: &async_nats::jetstream::Context,
    config: &NatsEventStoreConfig,
) -> TestResult<bool> {
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", "dedup-probe");
    Ok(js
        .publish_with_headers(
            config.transaction_subject("dedup-probe"),
            headers,
            b"probe".to_vec().into(),
        )
        .await?
        .await?
        .duplicate)
}

#[tokio::test]
async fn acceptance_survives_process_and_broker_restart_beyond_deduplication() -> TestResult {
    let server = Server(format!("rostfrei-acceptance-{}", std::process::id()));
    let mount = format!(
        "type=bind,src={}/../../scripts/nats-test.conf,dst=/etc/nats/test.conf,readonly",
        env!("CARGO_MANIFEST_DIR")
    );
    docker(&[
        "run",
        "--detach",
        "--name",
        &server.0,
        "--publish",
        "127.0.0.1::4222",
        "--mount",
        &mount,
        "nats:2.12.1-alpine@sha256:b3f2bd84176ae7bd0afa9c48a00f06d7d0818ff4aaee898e4172e0b8340e5816",
        "--config",
        "/etc/nats/test.conf",
    ])?;
    let url = format!("nats://{}", docker(&["port", &server.0, "4222/tcp"])?);
    let js = connect_ready(&url).await?;
    let config = NatsEventStoreConfig::for_bounded_context(&context()?)?;
    provision_event_store(&js, &config).await?;
    child(&url, "seed")?;
    check(
        !dedup_probe(&js, &config).await?,
        "first probe was deduplicated",
    )?;
    check(
        dedup_probe(&js, &config).await?,
        "broker did not deduplicate the second probe",
    )?;
    // The seed process shortened the broker window only after connecting with verified policy.
    tokio::time::sleep(Duration::from_secs(1)).await;
    docker(&["stop", "--time", "10", &server.0])?;
    docker(&["start", &server.0])?;
    let url = format!("nats://{}", docker(&["port", &server.0, "4222/tcp"])?);
    let js = connect_ready(&url).await?;
    check(
        !dedup_probe(&js, &config).await?,
        "broker duplicate evidence must have expired",
    )?;
    js.update_stream(config.stream_config()).await?;
    child(&url, "replay")?;
    child(&url, "failure")?;
    Ok(())
}

#[tokio::test]
#[ignore = "subprocess entry point invoked by acceptance_survives_process_and_broker_restart_beyond_deduplication"]
async fn acceptance_child() -> TestResult {
    let js = connect_ready(&std::env::var(URL_ENV)?).await?;
    let mode = std::env::var("ROSTFREI_ACCEPTANCE_RESTART_MODE")?;
    if mode == "failure" {
        return receipt_failure_does_not_ack(js).await;
    }
    let config = NatsEventStoreConfig::for_bounded_context(&context()?)?;
    let store = Arc::new(NatsEventStore::connect(js.clone(), config.clone()).await?);
    let calls = Arc::new(AtomicUsize::new(0));
    let bus = CommandBus::new(
        context()?,
        Arc::new(InMemoryMessagingAdapter::new(processor(
            store.clone(),
            calls.clone(),
        )?)),
    );
    let mut business_events = js
        .client()
        .subscribe(config.aggregate_subject_filter())
        .await?;
    js.client().flush().await?;
    if mode == "seed" {
        let mut policy = config.stream_config();
        policy.duplicate_window = Duration::from_millis(100);
        js.update_stream(policy).await?;
    }
    let before = js
        .get_stream(config.stream_name())
        .await?
        .cached_info()
        .state
        .messages;
    for (operation, guarded) in [("no-participants", false), ("read-only", true)] {
        verify_operation(&bus, &store, &calls, operation, guarded).await?;
    }
    check(
        calls.load(Ordering::Relaxed) == if mode == "seed" { 2 } else { 0 },
        "replay or changed evidence invoked the command handler",
    )?;
    check(
        store
            .load(&StreamId::new(
                AggregateType::new("guard")?,
                AggregateId::new("observed")?,
            ))
            .await?
            .is_empty(),
        "acceptance added aggregate history",
    )?;
    check(
        store
            .list_streams(&AggregateType::new("guard")?)
            .await?
            .is_empty(),
        "read guards created directory entries",
    )?;
    check(
        tokio::time::timeout(Duration::from_millis(100), business_events.next())
            .await
            .is_err(),
        "acceptance published a business event",
    )?;
    let after = js
        .get_stream(config.stream_name())
        .await?
        .cached_info()
        .state
        .messages;
    let expected = if mode == "seed" {
        before.checked_add(3).ok_or("message count overflow")?
    } else {
        before
    };
    check(
        after == expected,
        "acceptance or replay wrote unexpected records",
    )?;
    Ok(())
}

async fn verify_operation(
    bus: &CommandBus,
    store: &Arc<NatsEventStore>,
    calls: &Arc<AtomicUsize>,
    operation: &str,
    guarded: bool,
) -> TestResult {
    let outcome = bus
        .dispatch(request(operation, "original", guarded)?)
        .await?;
    check(
        outcome.response().outcome() == &CommandResponseOutcome::Accepted,
        "command was not accepted",
    )?;
    let receipt = store
        .load_transaction_receipt_in_context(context()?.name(), &OperationId::new(operation)?)
        .await?
        .ok_or("missing receipt")?;
    check(receipt.events().is_empty(), "acceptance contains events")?;
    check(
        receipt.streams().len() == usize::from(guarded),
        "unexpected guard count",
    )?;
    let encoded = bus.encode(request(operation, "original", guarded)?)?;
    let metadata =
        CommandExecutionMetadata::new(OperationId::new(operation)?, encoded.fingerprint())
            .with_bounded_context(context()?.name().clone())
            .with_correlation_id(CorrelationId::new("correlation")?)
            .with_causation_id(CausationId::new("cause")?);
    let replay = CommandExecutor::new(store.clone())
        .execute(
            &AcceptHandler(calls.clone()),
            metadata,
            &Accept {
                content: "original".to_owned(),
                guarded,
            },
        )
        .await?;
    check(
        replay == CommandOutcome::Accepted(CommandReceipt::ExactReplay(Vec::new())),
        "acceptance was not exactly replayed",
    )?;
    for changed in [
        request(operation, "changed", guarded)?,
        request(operation, "original", guarded)?
            .with_correlation_id(CorrelationId::new("changed")?),
        request(operation, "original", guarded)?.with_causation_id(CausationId::new("changed")?),
    ] {
        let response = bus.dispatch(changed).await?;
        let CommandResponseOutcome::Rejected(rejection) = response.response().outcome() else {
            return Err("changed evidence was accepted".into());
        };
        check(
            rejection.code().as_str() == "rostfrei.operation.identity-conflict",
            "changed evidence did not reach identity conflict checks",
        )?;
    }
    Ok(())
}

async fn receipt_failure_does_not_ack(js: async_nats::jetstream::Context) -> TestResult {
    // Distinct application subjects keep this capacity fixture independent of accepted operations.
    let capacity_context =
        ApplicationName::new("acceptance-capacity")?.bounded_context("acceptance")?;
    let config = NatsEventStoreConfig::new(&capacity_context, "acceptance_capacity")?
        .with_storage_limits(8192, 2048)?;
    provision_event_store(&js, &config).await?;
    let store = Arc::new(NatsEventStore::connect(js.clone(), config.clone()).await?);
    let mut exhausted = false;
    for index in 0..100 {
        let result = store
            .append_transaction(
                EventTransaction::new(
                    OperationId::new(format!("fill-{index}"))?,
                    ContentFingerprint::digest("fill"),
                    Vec::new(),
                )
                .with_bounded_context(capacity_context.name().clone()),
            )
            .await;
        if let Err(error) = result {
            assert_eq!(
                error.kind(),
                rostfrei_core::EventStoreErrorKind::CapacityExhausted
            );
            exhausted = true;
            break;
        }
    }
    assert!(exhausted);
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(store.clone(), calls.clone())?;
    let bus = CommandBus::new(
        capacity_context.clone(),
        Arc::new(InMemoryMessagingAdapter::new(processor.clone())),
    );
    let command = bus.encode(request("failed-acceptance", "original", false)?)?;
    assert!(processor.process(&command).await.is_err());
    let messaging = ApplicationMessagingConfig::new(capacity_context.application())?
        .with_max_bytes(64 * 1024 * 1024)?;
    provision_application_messaging(&js, &messaging).await?;
    let topology = messaging.topology();
    let handler = NatsMessagingAdapter::new(
        NatsPublisher::new(js.clone(), topology.clone()),
        NatsCommandResponseReader::new(js.clone(), topology.clone()),
    )
    .command_handler(processor);
    let delivery: MessageDelivery<CommandAddress> = MessageDelivery::new(
        command.address().clone(),
        command.message_id().clone(),
        command.payload().to_vec(),
        CallerMetadata::new(),
        DeliveryInfo::new(1, 0, 1, 1)?,
    )?;
    assert!(matches!(
        handler.handle(delivery).await,
        DeliveryDisposition::RetryAfter(_)
    ));
    assert_eq!(
        js.get_stream(topology.command_response_stream().as_str())
            .await?
            .cached_info()
            .state
            .messages,
        0
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(
        store
            .load_transaction_receipt_in_context(
                capacity_context.name(),
                &OperationId::new("failed-acceptance")?
            )
            .await?
            .is_none()
    );
    Ok(())
}

rostfrei::install_macro_support!();
