//! #85: standard HTTP router over a real private loopback socket, using a local bus.
#![allow(clippy::panic_in_result_fn, clippy::unwrap_used)]
use async_trait::async_trait;
use rostfrei::*;
use rostfrei_core::{Event, JsonEventCodec};
use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(rostfrei::BoundedContext)]
#[domain(id = "failure-contract", label = "Failure contract")]
struct FailureContract;
#[derive(rostfrei::Command)]
#[domain(context = FailureContract, id = "act", label = "Act")]
struct Act {
    target: String,
}
struct Target;
struct Changed;
impl Aggregate for Target {
    type State = ();
    type Event = Changed;
    const BOUNDED_CONTEXT: &'static str = "failure-contract";
    const AGGREGATE_TYPE: &'static str = "target";
    fn initial(_: &StreamId) {}
    fn apply((): &mut (), _: &Changed) {}
}
impl Event for Changed {
    fn event_type(&self) -> &'static str {
        "changed"
    }
    fn schema_version(&self) -> u32 {
        1
    }
    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        Ok(b"{}".to_vec())
    }
    fn decode_json(_: &RecordedEvent) -> Result<Self, EventCodecError> {
        Ok(Self)
    }
}
struct Handler;
#[async_trait]
impl CommandHandler<Act> for Handler {
    type Rejection = std::convert::Infallible;
    async fn handle(
        &self,
        command: &Act,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        execution
            .load::<Target>(&command.target)
            .await?
            .aggregate_mut()
            .raise(Changed);
        Ok(CommandDecision::Accepted)
    }
}
struct FaultStore {
    inner: InMemoryEventStore,
    kind: EventStoreErrorKind,
    active: Arc<AtomicBool>,
}
#[async_trait]
impl EventHistory for FaultStore {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        if self.active.load(Ordering::SeqCst) {
            return Err(EventStoreError::new(
                self.kind,
                "controlled HTTP aggregate load fault",
            ));
        }
        self.inner.load(id).await
    }
}
#[async_trait]
impl EventStore for FaultStore {
    async fn load_completed_outcome_in_context(
        &self,
        context: &BoundedContextName,
        operation: &OperationId,
    ) -> Result<Option<rostfrei_core::InlineCompletedOutcome>, EventStoreError> {
        self.inner
            .load_completed_outcome_in_context(context, operation)
            .await
    }
    async fn commit_outcome(
        &self,
        transaction: EventTransaction,
    ) -> Result<rostfrei_core::CompletedAppendOutcome, EventStoreError> {
        self.inner.commit_outcome(transaction).await
    }
    async fn append(
        &self,
        id: &StreamId,
        version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.inner.append(id, version, batch).await
    }
    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.inner.append_transaction(transaction).await
    }
    async fn load_transaction_receipt(
        &self,
        id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner.load_transaction_receipt(id).await
    }
    async fn load_transaction_receipt_in_context(
        &self,
        context: &BoundedContextName,
        id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.inner
            .load_transaction_receipt_in_context(context, id)
            .await
    }
}
#[async_trait]
impl OutcomeReader for FaultStore {
    async fn lookup_outcome(
        &self,
        key: &OutcomeKey,
    ) -> Result<OutcomeLookup<EncodedOutcomeDetails>, EventStoreError> {
        self.inner.lookup_outcome(key).await
    }
}
struct FaultSession<'a> {
    store: &'a FaultStore,
    inner: Box<dyn CommandCommitSession + 'a>,
}
#[async_trait]
impl EventHistory for FaultSession<'_> {
    async fn load(&self, id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        if self.store.active.load(Ordering::SeqCst) {
            return Err(EventStoreError::new(
                self.store.kind,
                "controlled HTTP aggregate load fault",
            ));
        }
        self.inner.load(id).await
    }
}
#[async_trait]
impl CommandCommitSession for FaultSession<'_> {
    async fn commit_outcome(
        self: Box<Self>,
        proposal: EventTransaction,
    ) -> Result<CompletedCommit, EventStoreError> {
        self.inner.commit_outcome(proposal).await
    }
}
#[async_trait]
impl CommandBackend for FaultStore {
    fn authority(&self) -> &CommandAuthority {
        self.inner.authority()
    }
    async fn begin_command(&self) -> Result<Box<dyn CommandCommitSession + '_>, EventStoreError> {
        Ok(Box::new(FaultSession {
            store: self,
            inner: self.inner.begin_command().await?,
        }))
    }
}
struct Server(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
#[derive(Clone, Copy)]
enum BadCodec {
    Decode,
    Encode,
}
impl EventCodec<Target> for BadCodec {
    fn encode(&self, event: &Changed, id: EventId) -> Result<NewEvent, EventCodecError> {
        if matches!(self, Self::Encode) {
            return Err(EventCodecError::new(
                EventCodecErrorKind::EncodingFailed,
                "permanent encoding failure",
            ));
        }
        <JsonEventCodec as EventCodec<Target>>::encode(&JsonEventCodec, event, id)
    }
    fn decode(&self, event: &RecordedEvent) -> Result<Changed, EventCodecError> {
        if matches!(self, Self::Decode) {
            return Err(EventCodecError::new(
                EventCodecErrorKind::MalformedPayload,
                "permanent decoding failure",
            ));
        }
        <JsonEventCodec as EventCodec<Target>>::decode(&JsonEventCodec, event)
    }
}
async fn http(
    kind: EventStoreErrorKind,
    codec: Option<BadCodec>,
) -> TestResult<(
    Server,
    std::net::SocketAddr,
    Arc<FaultStore>,
    BoundedContext,
)> {
    let context =
        ApplicationName::new("http-failure-contract")?.bounded_context("failure-contract")?;
    let store = Arc::new(FaultStore {
        inner: InMemoryEventStore::new(),
        kind,
        active: Arc::new(AtomicBool::new(codec.is_none())),
    });
    if matches!(codec, Some(BadCodec::Decode)) {
        let mut seed = CommandProcessor::new(context.name().clone(), store.clone());
        seed.register::<Act, _>(Handler, InfallibleCommandRejectionMapper)?;
        CommandBus::new(
            context.clone(),
            Arc::new(InMemoryMessagingAdapter::new(Arc::new(seed))),
        )
        .dispatch(CommandRequest::new(
            OperationId::new("seed")?,
            Act {
                target: "target".to_owned(),
            },
        ))
        .await?;
    }
    let mut processor = CommandProcessor::new(context.name().clone(), store.clone());
    processor.register::<Act, _>(Handler, InfallibleCommandRejectionMapper)?;
    if let Some(codec) = codec {
        processor.register_codec::<Target, _>(codec)?;
    }
    let bus = CommandBus::new(
        context.clone(),
        Arc::new(InMemoryMessagingAdapter::new(Arc::new(processor))),
    );
    let query = QueryBus::new(
        context.clone(),
        Arc::new(InMemoryQueryAdapter::new(Arc::new(QueryProcessor::new()))),
    );
    let mut registry = DomainRegistry::new();
    registry.register_command::<Act, Handler>()?;
    let router = rostfrei_http::router(
        Arc::new(registry),
        bus,
        query,
        rostfrei_http::HttpApiConfig::default(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = Server(tokio::spawn(
        async move { axum::serve(listener, router).await },
    ));
    Ok((server, address, store, context))
}
async fn submit(address: std::net::SocketAddr) -> TestResult<(u16, serde_json::Value)> {
    tokio::task::spawn_blocking(move || {
        let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.write_all(b"POST /contexts/failure-contract/commands/act/schemas/1 HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nIdempotency-Key: fault-operation\r\nConnection: close\r\nContent-Length: 19\r\n\r\n{\"target\":\"target\"}")?;
        let mut wire = String::new();
        socket.read_to_string(&mut wire)?;
        let (headers, body) = wire.split_once("\r\n\r\n").ok_or("invalid HTTP response framing")?;
        let status = headers.split_whitespace().nth(1).ok_or("missing HTTP status")?.parse()?;
        Ok((status, serde_json::from_str(body)?))
    }).await?
}
async fn permanent_http(codec: Option<BadCodec>) -> TestResult {
    let (_server, address, store, context) =
        http(EventStoreErrorKind::CorruptHistory, codec).await?;
    let (status, body) = submit(address).await?;
    assert!(
        store
            .inner
            .load_transaction_receipt_in_context(
                context.name(),
                &OperationId::new("fault-operation")?
            )
            .await?
            .is_none()
    );
    assert!(
        body.get("status").is_none(),
        "technical execution failure must not masquerade as accepted/rejected outcome"
    );
    assert_ne!(
        status, 503,
        "HTTP must preserve permanent vs transient execution classification"
    );
    assert_eq!(status, 500);
    let expected = match codec {
        None => serde_json::json!({"source":"store", "kind":"corrupt_history"}),
        Some(BadCodec::Decode) => serde_json::json!({"source":"codec", "kind":"malformed_payload"}),
        Some(BadCodec::Encode) => serde_json::json!({"source":"codec", "kind":"encoding_failed"}),
    };
    assert_eq!(body.get("execution"), Some(&expected));
    assert_eq!(
        body.get("recovery").and_then(serde_json::Value::as_str),
        Some("blocked")
    );
    Ok(())
}
#[tokio::test]
async fn corrupt_history_is_http_technical_failure_not_transient_unavailable_or_domain_rejection()
-> TestResult {
    permanent_http(None).await
}
#[tokio::test]
async fn codec_decode_is_http_permanent_technical_failure() -> TestResult {
    permanent_http(Some(BadCodec::Decode)).await
}
#[tokio::test]
async fn codec_encode_is_http_permanent_technical_failure() -> TestResult {
    permanent_http(Some(BadCodec::Encode)).await
}
#[tokio::test]
async fn unavailable_http_command_remains_retryable_and_same_operation_recovers() -> TestResult {
    let (_server, address, store, _) = http(EventStoreErrorKind::Unavailable, None).await?;
    let (status, body) = submit(address).await?;
    assert_eq!(status, 503);
    assert_eq!(
        body.get("execution"),
        Some(&serde_json::json!({"source":"store", "kind":"unavailable"}))
    );
    assert_eq!(
        body.get("recovery").and_then(serde_json::Value::as_str),
        Some("retry")
    );
    assert_eq!(
        body.get("code").and_then(serde_json::Value::as_str),
        Some("rostfrei.http.command-unavailable")
    );
    store.active.store(false, Ordering::SeqCst);
    let (status, body) = submit(address).await?;
    assert_eq!(status, 200);
    assert_eq!(
        body.get("status").and_then(serde_json::Value::as_str),
        Some("accepted")
    );
    Ok(())
}
rostfrei::install_macro_support!();
