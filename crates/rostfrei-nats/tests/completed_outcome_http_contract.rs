//! #92: actual standard HTTP/local -> native NATS replay, preserving the first wire evidence.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei::{
    CommandBusError, CommandBusObserver, CommandBusReceipt, CommandMessageAdapter, EncodedCommand,
};
use std::{
    io::{Read, Write},
    net::TcpStream,
};

struct Capture {
    inner: InMemoryMessagingAdapter,
    first: Arc<tokio::sync::Mutex<Option<EncodedCommand>>>,
}
#[async_trait]
impl CommandMessageAdapter for Capture {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        let mut first = self.first.lock().await;
        if first.is_none() {
            *first = Some(command.clone());
        }
        drop(first);
        self.inner.dispatch(command, observer).await
    }
}
struct Server(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn http(
    fixture: &Fixture,
    processor: Arc<CommandProcessor>,
) -> TestResult<(
    Server,
    std::net::SocketAddr,
    Arc<tokio::sync::Mutex<Option<EncodedCommand>>>,
)> {
    http_config(fixture, processor, rostfrei_http::HttpApiConfig::default()).await
}

async fn http_config(
    fixture: &Fixture,
    processor: Arc<CommandProcessor>,
    config: rostfrei_http::HttpApiConfig,
) -> TestResult<(
    Server,
    std::net::SocketAddr,
    Arc<tokio::sync::Mutex<Option<EncodedCommand>>>,
)> {
    let first = Arc::new(tokio::sync::Mutex::new(None));
    let bus = CommandBus::new(
        fixture.context.clone(),
        Arc::new(Capture {
            inner: InMemoryMessagingAdapter::new(processor),
            first: first.clone(),
        }),
    );
    let query = rostfrei::QueryBus::new(
        fixture.context.clone(),
        Arc::new(rostfrei::InMemoryQueryAdapter::new(Arc::new(
            rostfrei::QueryProcessor::new(),
        ))),
    );
    let mut registry = rostfrei::DomainRegistry::new();
    registry.register_command::<Act, Handler>()?;
    let router = rostfrei_http::router(Arc::new(registry), bus, query, config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    Ok((
        Server(tokio::spawn(
            async move { axum::serve(listener, router).await },
        )),
        address,
        first,
    ))
}

async fn submit_envelope(
    address: std::net::SocketAddr,
    operation: String,
    payload: Vec<u8>,
) -> TestResult<(u16, serde_json::Value)> {
    tokio::task::spawn_blocking(move || {
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        let headers = format!("POST /contexts/release-contract/commands/act/schemas/1/envelope HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nIdempotency-Key: {operation}\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", payload.len());
        socket.write_all(headers.as_bytes())?;
        socket.write_all(&payload)?;
        let mut response = String::new();
        socket.read_to_string(&mut response)?;
        let (headers, body) = response.split_once("\r\n\r\n").ok_or("invalid HTTP framing")?;
        Ok((headers.split_whitespace().nth(1).ok_or("missing HTTP status")?.parse()?, serde_json::from_str(body)?))
    }).await?
}

#[tokio::test]
async fn nats_rejection_replays_through_actual_http_with_unchanged_full_envelope() -> TestResult {
    let fixture = Fixture::new("92-nats-http-full").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let initial = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let _worker = fixture.command_worker(initial.clone()).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let command = request("rejected-original", "target", "add")?;
    let original = remote.encode(command.clone())?;
    let rejected_response = remote.dispatch(command).await?;
    assert!(rejected(rejected_response.response().outcome()));
    local_bus(&fixture.context, initial)
        .dispatch(request("repair-new-operation", "target", "create")?)
        .await?;
    let fresh = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let (_server, address, captured) = http_config(
        &fixture,
        fresh,
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .await?;
    let (status, body) = submit_envelope(
        address,
        original.operation_id().as_str().to_owned(),
        original.payload().to_vec(),
    )
    .await?;
    assert_eq!(
        captured.lock().await.as_ref(),
        Some(&original),
        "actual HTTP must preserve all envelope bytes and metadata, not regenerate created_at"
    );
    println!(
        "nats_to_http_original={}",
        String::from_utf8_lossy(original.payload())
    );
    assert_eq!(
        status, 404,
        "completed NATS rejection must remain authoritative after repair over actual HTTP"
    );
    assert_eq!(
        body.get("status").and_then(serde_json::Value::as_str),
        Some("rejected")
    );
    let CommandResponseOutcome::Rejected(error) = rejected_response.response().outcome() else {
        return Err("expected initial rejection".into());
    };
    assert_eq!(body.get("error"), Some(&serde_json::to_value(error)?));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "replay must not invoke the business handler"
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn accepted_no_event_replay_preserves_full_nats_envelope_through_actual_http() -> TestResult {
    let fixture = Fixture::new("92-accepted-http-full").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let initial = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let _worker = fixture.command_worker(initial).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let command = request("accepted-original", "target", "noop")?;
    let original = remote.encode(command.clone())?;
    assert_eq!(
        remote.dispatch(command).await?.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    let fresh = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let (_server, address, captured) = http_config(
        &fixture,
        fresh,
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .await?;
    let (status, body) = submit_envelope(
        address,
        original.operation_id().as_str().to_owned(),
        original.payload().to_vec(),
    )
    .await?;
    assert_eq!(status, 200);
    assert_eq!(
        body.get("status").and_then(serde_json::Value::as_str),
        Some("accepted")
    );
    assert_eq!(captured.lock().await.as_ref(), Some(&original));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(store.load(&stream("target")?).await?.is_empty());
    Ok(())
}

async fn lost_http_response(accepted: bool) -> TestResult {
    use tokio::io::AsyncReadExt;
    let fixture = Fixture::new(if accepted {
        "92-http-lost-accepted"
    } else {
        "92-http-lost-rejected"
    })
    .await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let first = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let original = local_bus(&fixture.context, first.clone()).encode(request(
        "http-uncertain-outcome",
        "target",
        if accepted { "create" } else { "add" },
    )?)?;
    let (_server, server_address, captured) = http_config(
        &fixture,
        first,
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let proxy = tokio::spawn(async move {
        let (client, _) = listener.accept().await?;
        let upstream = tokio::net::TcpStream::connect(server_address).await?;
        let (mut incoming, _outgoing) = client.into_split();
        let (mut response, mut request) = upstream.into_split();
        let upload = tokio::io::copy(&mut incoming, &mut request);
        let download = async {
            let mut discarded = Vec::new();
            response.read_to_end(&mut discarded).await?;
            std::io::Result::Ok(discarded)
        };
        tokio::select! { result = download => result, result = upload => { result?; Err(std::io::Error::other("client ended before response")) } }
    });
    assert!(
        submit_envelope(
            address,
            original.operation_id().as_str().to_owned(),
            original.payload().to_vec()
        )
        .await
        .is_err(),
        "client must actually lose the HTTP reply"
    );
    let discarded = proxy.await??;
    assert!(
        discarded.starts_with(if accepted {
            b"HTTP/1.1 200"
        } else {
            b"HTTP/1.1 404"
        }),
        "the completed server response, not just the request, was intercepted"
    );
    assert_eq!(captured.lock().await.as_ref(), Some(&original));
    let recreated = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    if !accepted {
        local_bus(&fixture.context, recreated.clone())
            .dispatch(request("http-uncertain-repair", "target", "create")?)
            .await?;
    }
    let before = calls.load(Ordering::SeqCst);
    let _worker = fixture.command_worker(recreated).await?;
    let replay = CommandBus::new(fixture.context.clone(), fixture.adapter.clone())
        .dispatch_encoded(original)
        .await?;
    assert_eq!(rejected(replay.response().outcome()), !accepted);
    assert_eq!(calls.load(Ordering::SeqCst), before);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn actual_http_accepted_response_loss_replays_over_nats_without_second_effect() -> TestResult
{
    lost_http_response(true).await
}
#[tokio::test]
async fn actual_http_rejected_response_loss_replays_over_nats_without_reevaluation() -> TestResult {
    lost_http_response(false).await
}

#[tokio::test]
async fn full_http_envelope_ingress_is_opt_in_and_rejects_changed_identity_before_dispatch()
-> TestResult {
    let fixture = Fixture::new("92-http-ingress-validation").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store, calls.clone(), None)?;
    let original = local_bus(&fixture.context, processor.clone()).encode(request(
        "validation",
        "target",
        "noop",
    )?)?;
    let (_disabled, address, captured) = http(&fixture, processor.clone()).await?;
    assert_eq!(
        submit_envelope(
            address,
            "validation".to_owned(),
            original.payload().to_vec()
        )
        .await?
        .0,
        404
    );
    assert!(captured.lock().await.is_none());
    let (_enabled, address, captured) = http_config(
        &fixture,
        processor,
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    )
    .await?;
    assert_eq!(
        submit_envelope(
            address,
            "different-operation".to_owned(),
            original.payload().to_vec()
        )
        .await?
        .0,
        400
    );
    let envelope: serde_json::Value = serde_json::from_slice(original.payload())?;
    for (path, value) in [
        (
            "payload",
            serde_json::json!({"command":"act", "schema_version":1, "payload":{"target":"different", "action":"noop", "item":"I"}}),
        ),
        ("created_at", serde_json::json!(253_402_300_800_000_u64)),
        ("correlation_id", serde_json::json!("different-correlation")),
        ("causation_id", serde_json::json!("different-cause")),
        ("message_id", serde_json::json!("fabricated-message-id")),
        ("schema_version", serde_json::json!(2)),
    ] {
        let mut changed = envelope.clone();
        *changed
            .get_mut(path)
            .ok_or("fixture omitted the tested identity dimension")? = value;
        assert_eq!(
            submit_envelope(
                address,
                "validation".to_owned(),
                serde_json::to_vec(&changed)?
            )
            .await?
            .0,
            400
        );
    }
    assert_eq!(
        submit_envelope(address, "validation".to_owned(), b"not-json".to_vec())
            .await?
            .0,
        400
    );
    assert!(
        captured.lock().await.is_none(),
        "invalid envelopes must fail before adapter or business invocation"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}
async fn submit(address: std::net::SocketAddr) -> TestResult<(u16, serde_json::Value)> {
    tokio::task::spawn_blocking(move || {
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        let payload = r#"{"target":"target","action":"add","item":"I"}"#;
        let wire = format!("POST /contexts/release-contract/commands/act/schemas/1 HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nIdempotency-Key: http-rejected-operation\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{payload}", payload.len());
        socket.write_all(wire.as_bytes())?;
        let mut response = String::new();
        socket.read_to_string(&mut response)?;
        let (headers, body) = response.split_once("\r\n\r\n").ok_or("invalid HTTP framing")?;
        Ok((headers.split_whitespace().nth(1).ok_or("missing HTTP status")?.parse()?, serde_json::from_str(body)?))
    }).await?
}
#[tokio::test]
async fn http_rejection_replays_through_nats_with_identical_canonical_evidence() -> TestResult {
    let fixture = Fixture::new("92-http-nats").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let processor = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let (_server, address, captured) = http(&fixture, processor.clone()).await?;
    let (status, body) = submit(address).await?;
    assert_eq!(status, 404);
    assert_eq!(
        body.get("status").and_then(serde_json::Value::as_str),
        Some("rejected")
    );
    let original = captured
        .lock()
        .await
        .clone()
        .ok_or("HTTP did not reach the real local command adapter")?;
    let envelope: CommandEnvelope<rostfrei::RoutedCommand> =
        serde_json::from_slice(original.payload())?;
    let mut same = CommandRequest::new(
        OperationId::new(original.operation_id().as_str())?,
        Act {
            target: "target".to_owned(),
            action: "add".to_owned(),
            item: "I".to_owned(),
        },
    )
    .with_correlation_id(envelope.correlation_id().clone())
    .with_created_at(envelope.created_at());
    if let Some(causation) = envelope.causation_id() {
        same = same.with_causation_id(causation.clone());
    }
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    assert_eq!(
        remote.encode(same.clone())?,
        original,
        "HTTP and NATS retries must use identical payload/context/correlation/causation/timestamp, not merely the same operation ID"
    );
    println!(
        "original_http_command={}",
        String::from_utf8_lossy(original.payload())
    );
    local_bus(&fixture.context, processor.clone())
        .dispatch(request("prerequisite", "target", "create")?)
        .await?;
    let _worker = fixture.command_worker(processor).await?;
    let replay = remote.dispatch(same).await?;
    assert!(
        rejected(replay.response().outcome()),
        "HTTP completed rejection must remain authoritative when retried over NATS after prerequisite repair"
    );
    let CommandResponseOutcome::Rejected(error) = replay.response().outcome() else {
        return Err("expected recorded rejection".into());
    };
    assert_eq!(
        serde_json::to_value(error)?,
        body.get("error")
            .ok_or("HTTP rejection omitted structured error")?
            .clone()
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    assert_eq!(
        remote
            .dispatch(request("new-attempt", "target", "add")?)
            .await?
            .response()
            .outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(store.load(&stream("target")?).await?.len(), 2);
    Ok(())
}
rostfrei::install_macro_support!();
