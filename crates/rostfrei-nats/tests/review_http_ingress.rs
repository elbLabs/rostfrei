//! Independent boundary controls for the opt-in actual HTTP route and encoded dispatcher.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
mod release_contract_support;
use async_trait::async_trait;
use release_contract_support::*;
use rostfrei_runtime::{
    CommandBusError, CommandBusErrorKind, CommandBusObserver, CommandBusReceipt,
    CommandMessageAdapter, EncodedCommand,
};

struct Capture {
    calls: Arc<AtomicUsize>,
    captured: Arc<tokio::sync::Mutex<Option<EncodedCommand>>>,
}
#[async_trait]
impl CommandMessageAdapter for Capture {
    async fn dispatch(
        &self,
        command: EncodedCommand,
        observer: Arc<dyn CommandBusObserver>,
    ) -> Result<CommandBusReceipt, CommandBusError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.captured.lock().await = Some(command.clone());
        observer
            .published(rostfrei_runtime::CommandPublication::new(
                command.message_id().clone(),
                false,
            ))
            .await;
        let response = CommandResponse::accepted(
            rostfrei_runtime::command_response_message_id(command.message_id()).unwrap(),
            command.message_id().clone(),
            command.address().clone(),
            command.operation_id().clone(),
            command.correlation_id().clone(),
        )
        .unwrap();
        Ok(CommandBusReceipt::new(false, response))
    }
}
struct Server(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn submit(
    address: std::net::SocketAddr,
    path: String,
    headers: String,
    payload: Vec<u8>,
) -> TestResult<(u16, String)> {
    tokio::task::spawn_blocking(move || {
        use std::io::{Read,Write};
        let mut socket=std::net::TcpStream::connect_timeout(&address,Duration::from_secs(3))?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        let wire=format!("POST {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n",payload.len());
        socket.write_all(wire.as_bytes())?;socket.write_all(&payload)?;
        let mut response=String::new();socket.read_to_string(&mut response)?;
        let (headers,body)=response.split_once("\r\n\r\n").ok_or("bad HTTP framing")?;
        assert!(headers.to_ascii_lowercase().contains("cache-control: private, no-store"));
        Ok((headers.split_whitespace().nth(1).ok_or("no status")?.parse()?,body.to_owned()))
    }).await?
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one socket fixture checks all ingress refusals before the final valid dispatch"
)]
async fn actual_envelope_route_limits_headers_scope_and_preserves_exact_bytes() -> TestResult {
    let app = ApplicationName::new("review-http")?;
    let context = app.bounded_context("release-contract")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let bus = CommandBus::new(
        context.clone(),
        Arc::new(Capture {
            calls: calls.clone(),
            captured: captured.clone(),
        }),
    );
    let original = bus.encode(request("operation", "target", "noop")?)?;
    let mut registry = rostfrei_runtime::DomainRegistry::new();
    registry.register_command::<Act, Handler>()?;
    let query = rostfrei_runtime::QueryBus::new(
        context.clone(),
        Arc::new(rostfrei_runtime::InMemoryQueryAdapter::new(Arc::new(
            rostfrei_runtime::QueryProcessor::new(),
        ))),
    );
    let config =
        rostfrei_http::HttpApiConfig::new(rostfrei_runtime::QueryOptions::default(), 1024)?
            .with_trusted_command_envelope_ingress();
    let router = rostfrei_http::router(Arc::new(registry), bus.clone(), query, config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = Server(tokio::spawn(
        async move { axum::serve(listener, router).await },
    ));
    let path = "/contexts/release-contract/commands/act/schemas/1/envelope";
    for (headers, status) in [
        ("Content-Type: application/json\r\n", 400),
        (
            "Content-Type: application/json\r\nIdempotency-Key: operation\r\nIdempotency-Key: operation\r\n",
            400,
        ),
        (
            "Content-Type: application/json\r\nContent-Type: application/json\r\nIdempotency-Key: operation\r\n",
            415,
        ),
        (
            "Content-Type: application/json; charset=utf-8\r\nIdempotency-Key: operation\r\n",
            415,
        ),
        (
            "Content-Type: application/json\r\nIdempotency-Key: operation!\r\n",
            400,
        ),
    ] {
        assert_eq!(
            submit(
                address,
                path.to_owned(),
                headers.to_owned(),
                original.payload().to_vec()
            )
            .await?
            .0,
            status
        );
    }
    let headers = "Content-Type: application/json\r\nIdempotency-Key: operation\r\n";
    assert_eq!(
        submit(
            address,
            path.to_owned(),
            headers.to_owned(),
            vec![b' '; 1025]
        )
        .await?
        .0,
        413
    );
    for path in [
        "/contexts/other/commands/act/schemas/1/envelope",
        "/contexts/release-contract/commands/act/schemas/2/envelope",
        "/contexts/release-contract/commands/unknown/schemas/1/envelope",
    ] {
        assert_eq!(
            submit(
                address,
                path.to_owned(),
                headers.to_owned(),
                original.payload().to_vec()
            )
            .await?
            .0,
            404
        );
    }
    // A valid envelope/ID from another application must be rejected, not rebound to this scope.
    let foreign = CommandBus::new(
        ApplicationName::new("other-app")?.bounded_context("release-contract")?,
        Arc::new(Capture {
            calls: calls.clone(),
            captured: captured.clone(),
        }),
    )
    .encode(request("operation", "target", "noop")?)?;
    assert_eq!(
        submit(
            address,
            path.to_owned(),
            headers.to_owned(),
            foreign.payload().to_vec()
        )
        .await?
        .0,
        400
    );
    let mut mutated: serde_json::Value = serde_json::from_slice(original.payload())?;
    mutated["payload"]["events_caused_by_command"] = serde_json::json!(true);
    assert_eq!(
        submit(
            address,
            path.to_owned(),
            headers.to_owned(),
            serde_json::to_vec(&mutated)?
        )
        .await?
        .0,
        400
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "all refusals precede dispatch"
    );
    let (status, body) = submit(
        address,
        path.to_owned(),
        headers.to_owned(),
        original.payload().to_vec(),
    )
    .await?;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "accepted"
    );
    assert_eq!(captured.lock().await.as_ref(), Some(&original));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

struct Observer(Arc<AtomicUsize>);
#[async_trait]
impl CommandBusObserver for Observer {
    async fn published(&self, _: rostfrei_runtime::CommandPublication) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn encoded_dispatch_refuses_invalid_evidence_before_adapter_or_publication_observer()
-> TestResult {
    let context = ApplicationName::new("review-dispatch")?.bounded_context("release-contract")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::new(AtomicUsize::new(0));
    let bus = CommandBus::new(
        context,
        Arc::new(Capture {
            calls: calls.clone(),
            captured: Arc::new(tokio::sync::Mutex::new(None)),
        }),
    );
    let original = bus.encode(request("operation", "target", "noop")?)?;
    let mut wire: serde_json::Value = serde_json::from_slice(original.payload())?;
    wire["correlation_id"] = serde_json::json!("changed");
    let changed = EncodedCommand::from_delivery(
        original.address().clone(),
        original.message_id().clone(),
        serde_json::to_vec(&wire)?,
    )?;
    assert_eq!(
        bus.dispatch_encoded_observed(changed, Arc::new(Observer(observed.clone())))
            .await
            .unwrap_err()
            .kind(),
        CommandBusErrorKind::InvalidMessage
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(observed.load(Ordering::SeqCst), 0);
    bus.dispatch_encoded_observed(original, Arc::new(Observer(observed.clone())))
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(observed.load(Ordering::SeqCst), 1);
    Ok(())
}
rostfrei_runtime::install_macro_support!();
