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
    let router = rostfrei_http::router(
        Arc::new(registry),
        bus,
        query,
        rostfrei_http::HttpApiConfig::default(),
    );
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
    let store = fixture.event_store().await?;
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
