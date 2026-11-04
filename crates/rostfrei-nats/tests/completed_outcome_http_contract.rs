//! Cross-transport replay preserves durable decisions and the original envelope.
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
rostfrei::install_macro_support!();

struct Capture {
    inner: InMemoryMessagingAdapter,
    command: Arc<tokio::sync::Mutex<Option<EncodedCommand>>>,
}
#[async_trait]
impl CommandMessageAdapter for Capture {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        *self.command.lock().await = Some(command.clone());
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
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let bus = CommandBus::new(
        fixture.context.clone(),
        Arc::new(Capture {
            inner: InMemoryMessagingAdapter::new(processor),
            command: captured.clone(),
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
        rostfrei_http::HttpApiConfig::default().with_trusted_command_envelope_ingress(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    Ok((
        Server(tokio::spawn(
            async move { axum::serve(listener, router).await },
        )),
        address,
        captured,
    ))
}
async fn submit(
    address: std::net::SocketAddr,
    original: &EncodedCommand,
) -> TestResult<(u16, serde_json::Value)> {
    let operation = original.operation_id().as_str().to_owned();
    let payload = original.payload().to_vec();
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
async fn nats_rejection_replays_through_http_after_business_state_changes() -> TestResult {
    let fixture = Fixture::new("nats-http-rejection").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let initial = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let _worker = fixture.command_worker(initial.clone()).await?;
    let remote = CommandBus::new(fixture.context.clone(), fixture.adapter.clone());
    let original = remote.encode(request("rejected-original", "target", "add")?)?;
    let response = remote.dispatch_encoded(original.clone()).await?;
    let CommandResponseOutcome::Rejected(error) = response.response().outcome() else {
        return Err("expected initial rejection".into());
    };
    local_bus(&fixture.context, initial)
        .dispatch(request("repair", "target", "create")?)
        .await?;
    let fresh = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let (_server, address, captured) = http(&fixture, fresh).await?;
    let (status, body) = submit(address, &original).await?;
    assert_eq!(status, 404);
    assert_eq!(body.get("error"), Some(&serde_json::to_value(error)?));
    assert_eq!(captured.lock().await.as_ref(), Some(&original));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn lost_http_acceptance_replays_over_nats_without_a_second_effect() -> TestResult {
    use tokio::io::AsyncReadExt;
    let fixture = Fixture::new("http-nats-lost-response").await?;
    let store = fixture.split_event_store().await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let first = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let original = local_bus(&fixture.context, first.clone()).encode(request(
        "uncertain",
        "target",
        "create",
    )?)?;
    let (_server, server_address, captured) = http(&fixture, first).await?;
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
    assert!(submit(address, &original).await.is_err());
    let discarded = tokio::time::timeout(WAIT, proxy).await???;
    assert!(
        discarded.starts_with(b"HTTP/1.1 200"),
        "the committed reply was actually intercepted"
    );
    assert_eq!(captured.lock().await.as_ref(), Some(&original));
    let recreated = processor(&fixture.context, store.clone(), calls.clone(), None)?;
    let _worker = fixture.command_worker(recreated).await?;
    let replay = CommandBus::new(fixture.context.clone(), fixture.adapter.clone())
        .dispatch_encoded(original)
        .await?;
    assert_eq!(
        replay.response().outcome(),
        &CommandResponseOutcome::Accepted
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.load(&stream("target")?).await?.len(), 1);
    Ok(())
}
