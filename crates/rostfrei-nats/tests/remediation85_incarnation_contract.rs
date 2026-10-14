//! Native disposition and actual PubAck retention controls for the administrative race.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use release_contract_support::*;
use rostfrei::EncodedCommand;

#[derive(Clone, Copy, Eq, PartialEq)]
enum GatePoint {
    Publication,
    PubAck,
}

struct Proxy(tokio::task::JoinHandle<TestResult>);
impl Drop for Proxy {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn gate_proxy(
    subject: String,
    stream: String,
    point: GatePoint,
) -> TestResult<(
    Proxy,
    std::net::SocketAddr,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Semaphore>,
)> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = std::env::var("ROSTFREI_NATS_URL")?;
    let upstream = url.rsplit('@').next().ok_or("missing upstream")?.to_owned();
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let notify = reached.clone();
    let gate = release.clone();
    let task = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(upstream).await?;
        let (client_read, mut client_write) = client.into_split();
        let (upstream_read, mut upstream_write) = upstream.into_split();
        let upload = async {
            let mut reader = BufReader::new(client_read);
            let mut gated = false;
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).await? == 0 {
                    return TestResult::Ok(());
                }
                let text = std::str::from_utf8(&line)?;
                if text.starts_with("PUB ") || text.starts_with("HPUB ") {
                    let parts: Vec<_> = text.split_whitespace().collect();
                    let length: usize = parts.last().ok_or("no publication length")?.parse()?;
                    let mut body =
                        vec![0; length.checked_add(2).ok_or("publication length overflow")?];
                    reader.read_exact(&mut body).await?;
                    if point == GatePoint::Publication
                        && !gated
                        && parts.get(1) == Some(&subject.as_str())
                    {
                        gated = true;
                        notify.notify_one();
                        gate.acquire().await?.forget();
                    }
                    upstream_write.write_all(&line).await?;
                    upstream_write.write_all(&body).await?;
                } else {
                    upstream_write.write_all(&line).await?;
                }
            }
        };
        let download = async {
            let mut reader = BufReader::new(upstream_read);
            let mut gated = false;
            let ack = format!("\"stream\":\"{stream}\"");
            loop {
                let mut line = Vec::new();
                if reader.read_until(b'\n', &mut line).await? == 0 {
                    return TestResult::Ok(());
                }
                let text = std::str::from_utf8(&line)?;
                if text.starts_with("MSG ") || text.starts_with("HMSG ") {
                    let length: usize = text
                        .split_whitespace()
                        .last()
                        .ok_or("no message length")?
                        .parse()?;
                    let mut body = vec![0; length.checked_add(2).ok_or("message length overflow")?];
                    reader.read_exact(&mut body).await?;
                    if point == GatePoint::PubAck
                        && !gated
                        && body
                            .windows(ack.len())
                            .any(|window| window == ack.as_bytes())
                    {
                        gated = true;
                        notify.notify_one();
                        gate.acquire().await?.forget();
                    }
                    client_write.write_all(&line).await?;
                    client_write.write_all(&body).await?;
                } else {
                    client_write.write_all(&line).await?;
                }
            }
        };
        tokio::select! { result=upload => result, result=download => result }
    });
    Ok((Proxy(task), address, reached, release))
}

async fn publisher(
    f: &Fixture,
    c: &EncodedCommand,
    point: GatePoint,
) -> TestResult<(
    Proxy,
    async_nats::Client,
    NatsPublisher,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Semaphore>,
)> {
    let (proxy, address, reached, release) = gate_proxy(
        c.response_address()?.to_string(),
        f.messaging
            .topology()
            .command_response_stream()
            .as_str()
            .to_owned(),
        point,
    )
    .await?;
    let client = async_nats::ConnectOptions::with_user_and_password(
        std::env::var("ROSTFREI_CONTRACT_NATS_USER")?,
        std::env::var("ROSTFREI_CONTRACT_NATS_PASSWORD")?,
    )
    .connect(format!("nats://{address}"))
    .await?;
    let publisher = NatsPublisher::new(
        async_nats::jetstream::new(client.clone()),
        f.messaging.topology().clone(),
    );
    Ok((proxy, client, publisher, reached, release))
}

fn command(f: &Fixture, target: &str) -> TestResult<EncodedCommand> {
    local_bus(
        &f.context,
        processor(
            &f.context,
            Arc::new(InMemoryEventStore::new()),
            Arc::new(AtomicUsize::new(0)),
            None,
        )?,
    )
    .encode(request("operation", target, "create")?)
    .map_err(Into::into)
}

#[tokio::test]
async fn native_handler_does_not_select_blocked_quarantine_for_an_old_epoch_puback() -> TestResult {
    let f = Fixture::new("protocol-native-epoch").await?;
    let c = command(&f, "bad-target")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(FaultStore {
        inner: InMemoryEventStore::new(),
        kind: EventStoreErrorKind::CorruptHistory,
        active: Arc::new(AtomicBool::new(true)),
    });
    let processor = processor(&f.context, store.clone(), calls.clone(), None)?;
    let (proxy, client, publisher, reached, release) =
        publisher(&f, &c, GatePoint::Publication).await?;
    let adapter = NatsMessagingAdapter::new(
        publisher,
        NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone()),
    );
    let handler = adapter.command_handler(processor);
    let delivery = command_delivery(&c)?;
    let task = tokio::spawn(async move { handler.handle(delivery).await });
    tokio::time::timeout(Duration::from_secs(3), reached.notified()).await?;
    let original =
        f.js.get_stream(f.messaging.topology().command_response_stream().as_str())
            .await?;
    assert_eq!(original.cached_info().state.messages, 0);
    let config = original.cached_info().config.clone();
    f.js.delete_stream(f.messaging.topology().command_response_stream().as_str())
        .await?;
    let recreated = f.js.create_stream(config).await?;
    assert_ne!(
        original.cached_info().created,
        recreated.cached_info().created
    );
    release.add_permits(1);
    let disposition = task.await?;
    let retained = recreated
        .get_last_raw_message_by_subject(c.response_address()?.as_str())
        .await?;
    let response: CommandResponse = serde_json::from_slice(&retained.payload)?;
    assert_eq!(
        response.outcome(),
        &CommandResponseOutcome::ExecutionFailed(ExecutionFailure::Store(
            StoreFailureKind::CorruptHistory
        ))
    );
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    let read = reader
        .find_command_response(
            &c.response_address()?,
            c.operation_id(),
            c.message_id(),
            Duration::from_secs(2),
        )
        .await;
    println!(
        "native gated epoch race: disposition={disposition:?}, retained seq={}, reader={read:?}",
        retained.sequence
    );
    assert!(read.is_err());
    assert!(
        matches!(disposition, DeliveryDisposition::RetryAfter(_)),
        "failed usable-evidence check must not select blocked quarantine from a fresh PubAck alone"
    );
    assert_eq!(
        recreated.get_raw_message(retained.sequence).await?.payload,
        retained.payload,
        "unknown retained record must remain for audit"
    );
    assert!(f.quarantine().await?.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(store.inner.load(&stream("bad-target")?).await?.is_empty());
    assert!(
        store
            .load_transaction_receipt_in_context(f.context.name(), &OperationId::new("operation")?)
            .await?
            .is_none()
    );
    client.drain().await?;
    drop(proxy);
    Ok(())
}

#[tokio::test]
async fn fresh_real_puback_cannot_establish_usable_durability_after_its_record_was_deleted()
-> TestResult {
    let f = Fixture::new("protocol-puback-retention").await?;
    let c = command(&f, "target")?;
    let failure = ExecutionFailure::Store(StoreFailureKind::Unavailable);
    let response = CommandResponse::execution_failed(
        rostfrei::command_technical_response_message_id(c.message_id(), failure)?,
        c.message_id().clone(),
        c.address().clone(),
        c.operation_id().clone(),
        c.correlation_id().clone(),
        failure,
    )?;
    let message = OutboundMessage::json(
        c.response_address()?,
        response.message_id().clone(),
        &response,
    )?;
    let (proxy, client, publisher, reached, release) = publisher(&f, &c, GatePoint::PubAck).await?;
    let prepared = publisher
        .prepare_command_response(message.clone(), Duration::from_secs(2))
        .await?;
    let task = tokio::spawn(async move {
        publisher
            .publish_prepared_command_response(&prepared, Duration::from_secs(5))
            .await
    });
    // This gate holds the actual broker Ack, AFTER the record was stored, not a fabricated result.
    tokio::time::timeout(Duration::from_secs(3), reached.notified()).await?;
    let stream =
        f.js.get_stream(f.messaging.topology().command_response_stream().as_str())
            .await?;
    let retained = stream
        .get_last_raw_message_by_subject(c.response_address()?.as_str())
        .await?;
    assert_eq!(retained.payload.as_ref(), message.payload());
    let snapshot = serde_json::json!({"subject":retained.subject.to_string(), "sequence":retained.sequence, "headers":retained.headers, "payload_utf8":std::str::from_utf8(&retained.payload)?});
    let path = std::path::PathBuf::from(std::env::var("ROSTFREI_CONTRACT_EVIDENCE_DIR")?)
        .join("puback-record-before-owned-deletion.json");
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(&serde_json::to_vec_pretty(&snapshot)?)?;
    assert!(stream.delete_message(retained.sequence).await?);
    release.add_permits(1);
    let result = task.await?;
    println!(
        "real PubAck held while owned admin removed seq={}: publication={result:?}",
        retained.sequence
    );
    assert!(
        result.is_err(),
        "fresh Ack without retained usable evidence is not success"
    );
    let reader = NatsCommandResponseReader::new(f.js.clone(), f.messaging.topology().clone());
    assert!(
        reader
            .find_command_response(
                &c.response_address()?,
                c.operation_id(),
                c.message_id(),
                Duration::from_secs(2)
            )
            .await?
            .is_none()
    );
    client.drain().await?;
    drop(proxy);
    Ok(())
}

rostfrei::install_macro_support!();
