#![allow(dead_code)]

#[path = "../src/command_response.rs"]
mod command_response;
#[path = "../src/connection.rs"]
mod connection;
#[path = "../src/consumer.rs"]
mod consumer;
#[path = "../src/error.rs"]
mod error;
#[path = "../src/hex.rs"]
mod hex;
#[path = "../src/messaging_adapter.rs"]
mod messaging_adapter;
#[path = "../src/messaging_config.rs"]
mod messaging_config;
#[path = "../src/provisioning.rs"]
mod provisioning;
#[path = "../src/publish.rs"]
mod publish;
#[path = "../src/query.rs"]
mod query;
#[path = "../src/stream_policy.rs"]
mod stream_policy;

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use base64::Engine as _;
use rostfrei_messaging_core::{
    ApplicationErrorCode, ApplicationName, BoundedContext, CallerMetadata, CommandAddress,
    CommandPublisher, CommandResponse, CommandResponseReadErrorKind, CommandResponseReader,
    ConsumerConfig, CorrelationId, DeliveryDisposition, EnvelopeContext, MessageConsumerFactory,
    MessageDelivery, MessageHandler, MessageId, MessageTimestamp, OperationId, OutboundMessage,
    QuarantineReason, QueryAddress, QueryErrorClassification, QueryErrorPayload, QueryHandler,
    QueryOptions, QueryOutcome, QueryRequest, QueryRequestErrorKind, QueryRequester, QueryResponse,
    QueryServer, QueryServerError, QueryServerErrorKind, RetryDelay, SchemaVersion, TraceContext,
    derive_command_response_address,
};
use serde_json::{Value, json};

use command_response::NatsCommandResponseReader;
use connection::{NatsConnection, connect};
use consumer::{NatsConsumerFactory, QuarantineRecord};
use messaging_config::{MessagingTopology, NatsConnectionConfig, QueueGroup, StreamName};
use provisioning::{
    ApplicationMessagingConfig, provision_application_messaging, provision_durable_consumer,
};
use publish::{CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE, NatsPublishAck, NatsPublisher};
use query::{CORRELATION_ID_HEADER, NatsQueryServerConfig, REQUEST_ID_HEADER};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn checked_add_usize(value: usize, increment: usize, context: &'static str) -> TestResult<usize> {
    value
        .checked_add(increment)
        .ok_or_else(|| format!("{context} exceeds usize").into())
}

const TEST_NATS_URL_ENV: &str = "ROSTFREI_NATS_URL";
const TRACE_PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    connection: NatsConnection,
    topology: MessagingTopology,
    context: BoundedContext,
}

impl Fixture {
    async fn new(url: String) -> TestResult<Self> {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let application = ApplicationName::new(format!("test-{}-{sequence}", std::process::id()))?;
        let context = application.bounded_context("messaging")?;
        let messaging =
            ApplicationMessagingConfig::new(&application)?.with_max_bytes(64 * 1024 * 1024)?;
        let topology = messaging.topology().clone();
        let connection = connect(&NatsConnectionConfig::new(
            format!("messaging-test-{sequence}"),
            url,
        ))
        .await?;

        provision_application_messaging(connection.jetstream(), &messaging).await?;
        provision_application_messaging(connection.jetstream(), &messaging).await?;
        connection.verify_application_messaging(&messaging).await?;

        Ok(Self {
            connection,
            topology,
            context,
        })
    }

    fn command_address(&self, name: &str) -> TestResult<CommandAddress> {
        Ok(self.context.command_address(name)?)
    }

    fn query_address(&self, name: &str) -> TestResult<QueryAddress> {
        Ok(self.context.query_address(name)?)
    }

    async fn message_counts(&self) -> TestResult<[u64; 4]> {
        Ok([
            stream_message_count(self.connection.jetstream(), self.topology.command_stream())
                .await?,
            stream_message_count(
                self.connection.jetstream(),
                self.topology.command_response_stream(),
            )
            .await?,
            stream_message_count(
                self.connection.jetstream(),
                self.topology.integration_event_stream(),
            )
            .await?,
            stream_message_count(
                self.connection.jetstream(),
                self.topology.quarantine_stream(),
            )
            .await?,
        ])
    }

    async fn cleanup(self) -> TestResult<()> {
        for stream in [
            self.topology.command_stream(),
            self.topology.command_response_stream(),
            self.topology.integration_event_stream(),
            self.topology.quarantine_stream(),
        ] {
            self.connection
                .jetstream()
                .delete_stream(stream.as_str())
                .await?;
        }
        self.connection.drain().await?;
        Ok(())
    }
}

#[tokio::test]
async fn immutable_command_response_roundtrip_reconciles_duplicates_and_conflicts() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await.expect("messaging fixture");
    let command_address = fixture
        .command_address("durable-response")
        .expect("durable response command address");
    let operation_id = OperationId::new("operation-1").expect("operation id");
    let command_message_id =
        message_id("command-response-source").expect("command response source message id");
    let response_address =
        derive_command_response_address(&command_address, &operation_id, &command_message_id)
            .expect("response address");
    let response = CommandResponse::accepted(
        message_id("command-response").expect("command response message id"),
        command_message_id.clone(),
        command_address.clone(),
        operation_id.clone(),
        CorrelationId::new("response-correlation").expect("correlation id"),
    )
    .expect("accepted response");
    let message = OutboundMessage::json(
        response_address.clone(),
        response.message_id().clone(),
        &response,
    )
    .expect("response message");
    let publisher = fixture.connection.publisher(fixture.topology.clone());

    let first = publisher
        .publish_command_response_with_ack(message.clone(), Duration::from_secs(5))
        .await
        .expect("first response PubAck");
    assert_eq!(first.stream(), fixture.topology.command_response_stream());
    assert!(!first.duplicate());
    let duplicate = publisher
        .publish_command_response_with_ack(message, Duration::from_secs(5))
        .await
        .expect("matching response duplicate");
    assert!(duplicate.duplicate());
    assert_eq!(duplicate.sequence(), first.sequence());

    let conflicting = CommandResponse::accepted(
        response.message_id().clone(),
        command_message_id.clone(),
        command_address.clone(),
        operation_id.clone(),
        CorrelationId::new("different-correlation").expect("correlation id"),
    )
    .expect("conflicting response");
    let conflict = publisher
        .publish_command_response_with_ack(
            OutboundMessage::json(
                response_address.clone(),
                conflicting.message_id().clone(),
                &conflicting,
            )
            .expect("conflicting response message"),
            Duration::from_secs(5),
        )
        .await
        .expect_err("a response subject is immutable");
    assert_eq!(conflict, error::NatsError::IdentityConflict);

    let reader = fixture
        .connection
        .command_response_reader(fixture.topology.clone());
    let read = reader
        .read_command_response(
            &response_address,
            &operation_id,
            &command_message_id,
            Duration::from_secs(2),
        )
        .await
        .expect("stored response");
    assert_eq!(read, response);
    let identity_error = reader
        .read_command_response(
            &response_address,
            &OperationId::new("different-operation").expect("operation id"),
            &command_message_id,
            Duration::from_secs(2),
        )
        .await
        .expect_err("expected operation identity must match");
    assert_eq!(
        identity_error.kind(),
        CommandResponseReadErrorKind::IdentityConflict
    );

    assert_absent_response_times_out(&fixture, &reader, &command_address)
        .await
        .expect("absent response assertion");

    fixture.cleanup().await.expect("messaging fixture cleanup");
}

async fn assert_absent_response_times_out(
    fixture: &Fixture,
    reader: &command_response::NatsCommandResponseReader,
    command_address: &CommandAddress,
) -> TestResult<()> {
    let operation_id = OperationId::new("absent-operation")?;
    let command_message_id = message_id("absent-command")?;
    let address =
        derive_command_response_address(command_address, &operation_id, &command_message_id)?;
    let result = reader
        .read_command_response(
            &address,
            &operation_id,
            &command_message_id,
            Duration::from_millis(100),
        )
        .await;
    let Err(error) = result else {
        return Err("absent response unexpectedly found".into());
    };
    assert_eq!(error.kind(), CommandResponseReadErrorKind::Timeout);
    assert_eq!(
        stream_message_count(
            fixture.connection.jetstream(),
            fixture.topology.command_response_stream(),
        )
        .await?,
        1
    );
    Ok(())
}

fn test_url() -> Option<String> {
    std::env::var(TEST_NATS_URL_ENV).ok()
}

fn message_id(prefix: &str) -> TestResult<MessageId> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(MessageId::new(format!("{prefix}-{nanos}"))?)
}

fn query_request(id: MessageId, payload: Value) -> TestResult<QueryRequest<Value>> {
    Ok(QueryRequest::new(
        EnvelopeContext::new(
            id,
            SchemaVersion::new(1)?,
            CorrelationId::new("test-correlation")?,
            None,
        ),
        MessageTimestamp::from_unix_milliseconds(1_700_000_000_000)?,
        CallerMetadata::new(),
        Some(TraceContext::new(TRACE_PARENT)?),
        payload,
    )?)
}

async fn stream_message_count(
    context: &async_nats::jetstream::Context,
    name: &StreamName,
) -> TestResult<u64> {
    let mut stream = context.get_stream(name.as_str()).await?;
    Ok(stream.info().await?.state.messages)
}

struct ApplicationScopeGuardResults {
    policy_verification: Result<(), error::NatsError>,
    cross_application_publish: Result<NatsPublishAck, error::NatsError>,
}

async fn application_scope_guard_results(
    fixture: &Fixture,
    publisher: &NatsPublisher,
) -> TestResult<ApplicationScopeGuardResults> {
    let mismatched_policy = ApplicationMessagingConfig::new(fixture.context.application())?
        .with_max_bytes(32 * 1024 * 1024)?;
    let policy_verification = fixture
        .connection
        .verify_application_messaging(&mismatched_policy)
        .await;

    let cross_application = OutboundMessage::new(
        CommandAddress::new("other", "messaging", "publish")?,
        message_id("cross-application")?,
        br#"{"ok":true}"#.to_vec(),
    )?;
    let cross_application_publish = publisher
        .publish_command_with_ack(cross_application, Duration::from_secs(5))
        .await;
    Ok(ApplicationScopeGuardResults {
        policy_verification,
        cross_application_publish,
    })
}

#[tokio::test]
async fn puback_confirms_stream_sequence_duplicate_and_owned_headers() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await.expect("messaging fixture");
    let address = fixture
        .command_address("publish")
        .expect("publish command address");
    let mut metadata = CallerMetadata::new();
    metadata
        .insert("x-test-metadata", "caller-value")
        .expect("safe metadata");
    assert!(metadata.insert("Nats-Msg-Id", "override").is_err());
    let id = message_id("publish").expect("publish message id");
    let message = OutboundMessage::new(address.clone(), id.clone(), br#"{"ok":true}"#.to_vec())
        .expect("outbound message")
        .with_metadata(metadata)
        .with_trace_context(TraceContext::new(TRACE_PARENT).expect("trace context"));
    let publisher = NatsPublisher::new(
        fixture.connection.jetstream().clone(),
        fixture.topology.clone(),
    );
    let scope_guards = application_scope_guard_results(&fixture, &publisher)
        .await
        .expect("application scope guard observations");
    assert!(matches!(
        scope_guards.policy_verification,
        Err(error::NatsError::Configuration)
    ));
    assert!(matches!(
        scope_guards.cross_application_publish,
        Err(error::NatsError::InvalidMessage)
    ));

    let first = publisher
        .publish_command_with_ack(message.clone(), Duration::from_secs(5))
        .await
        .expect("first PubAck");
    assert_eq!(first.stream(), fixture.topology.command_stream());
    assert!(first.sequence() > 0);
    assert!(!first.duplicate());
    let duplicate = publisher
        .publish_command_with_ack(message, Duration::from_secs(5))
        .await
        .expect("duplicate PubAck");
    assert!(duplicate.duplicate());
    assert_eq!(duplicate.sequence(), first.sequence());

    let stream = fixture
        .connection
        .jetstream()
        .get_stream(fixture.topology.command_stream().as_str())
        .await
        .expect("command stream");
    let stored = stream
        .get_raw_message(first.sequence())
        .await
        .expect("stored command");
    assert_eq!(
        stored
            .headers
            .get(CONTENT_TYPE_HEADER)
            .map(async_nats::HeaderValue::as_str),
        Some(JSON_CONTENT_TYPE)
    );
    assert_eq!(
        stored
            .headers
            .get("Nats-Msg-Id")
            .map(async_nats::HeaderValue::as_str),
        Some(id.as_str())
    );
    assert_eq!(
        stored
            .headers
            .get("Nats-Expected-Stream")
            .map(async_nats::HeaderValue::as_str),
        Some(fixture.topology.command_stream().as_str())
    );
    assert_eq!(
        stored
            .headers
            .get("x-test-metadata")
            .map(async_nats::HeaderValue::as_str),
        Some("caller-value")
    );
    assert_eq!(
        stored
            .headers
            .get("traceparent")
            .map(async_nats::HeaderValue::as_str),
        Some(TRACE_PARENT)
    );

    fixture.cleanup().await.expect("messaging fixture cleanup");
}

struct DispositionHandler {
    delivered_message_ids: Mutex<HashSet<String>>,
    acknowledged: AtomicUsize,
    retry_delay: RetryDelay,
    quarantine_reason: QuarantineReason,
}

impl DispositionHandler {
    fn new() -> TestResult<Self> {
        Ok(Self {
            delivered_message_ids: Mutex::new(HashSet::new()),
            acknowledged: AtomicUsize::new(0),
            retry_delay: RetryDelay::new(Duration::from_millis(100))?,
            quarantine_reason: QuarantineReason::new("test quarantine")?,
        })
    }
}

#[async_trait]
impl MessageHandler<CommandAddress> for DispositionHandler {
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        let id = delivery.message_id().as_str().to_owned();
        let mut delivered_message_ids = match self.delivered_message_ids.lock() {
            Ok(delivered_message_ids) => delivered_message_ids,
            Err(poisoned) => poisoned.into_inner(),
        };
        let first_delivery = delivered_message_ids.insert(id.clone());
        drop(delivered_message_ids);
        match id.as_str() {
            value if value.starts_with("ack-") => {
                self.acknowledged.fetch_add(1, Ordering::Relaxed);
                DeliveryDisposition::Acknowledge
            }
            value if value.starts_with("retry-") && first_delivery => {
                DeliveryDisposition::RetryAfter(self.retry_delay)
            }
            value if value.starts_with("retry-") => {
                self.acknowledged.fetch_add(1, Ordering::Relaxed);
                DeliveryDisposition::Acknowledge
            }
            _ => DeliveryDisposition::Quarantine(self.quarantine_reason.clone()),
        }
    }
}

struct ProvisionedDispositionConsumer {
    config: ConsumerConfig<CommandAddress>,
    ack_wait: Duration,
}

async fn provision_disposition_consumer(
    fixture: &Fixture,
    address: &CommandAddress,
) -> TestResult<ProvisionedDispositionConsumer> {
    let config = ConsumerConfig::new(
        fixture.context.consumer_name("consume", 1)?,
        fixture.context.durable_name("consume", 1)?,
        address.clone(),
        Duration::from_secs(10),
        Duration::from_secs(5),
        4,
        3,
    )?;
    let ack_wait =
        provision_durable_consumer(fixture.connection.jetstream(), &fixture.topology, &config)
            .await?
            .config
            .ack_wait;
    provision_durable_consumer(fixture.connection.jetstream(), &fixture.topology, &config).await?;
    Ok(ProvisionedDispositionConsumer { config, ack_wait })
}

async fn publish_disposition_commands(
    publisher: &NatsPublisher,
    address: &CommandAddress,
) -> TestResult<MessageId> {
    let quarantine_id = message_id("quarantine")?;
    for (id, prefix) in [
        (message_id("ack")?, "ack"),
        (message_id("retry")?, "retry"),
        (quarantine_id.clone(), "quarantine"),
    ] {
        let mut metadata = CallerMetadata::new();
        if prefix == "quarantine" {
            metadata.insert("x-quarantine-test", "preserved")?;
        }
        let mut message = OutboundMessage::new(
            address.clone(),
            id,
            format!(r#"{{"kind":"{prefix}"}}"#).into_bytes(),
        )?
        .with_metadata(metadata);
        if prefix == "quarantine" {
            message = message.with_correlation_id(CorrelationId::new("quarantine-correlation")?);
        }
        publisher.publish_command(message).await?;
    }
    Ok(quarantine_id)
}

#[tokio::test]
async fn durable_consumer_applies_ack_retry_and_puback_before_quarantine_term() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await.expect("messaging fixture");
    let address = fixture
        .command_address("consume")
        .expect("consume command address");
    let provisioned = provision_disposition_consumer(&fixture, &address)
        .await
        .expect("provision durable consumer idempotently");
    assert_eq!(provisioned.ack_wait, Duration::from_secs(10));

    let handler = Arc::new(DispositionHandler::new().expect("disposition handler"));
    let factory = NatsConsumerFactory::new(
        fixture.connection.jetstream().clone(),
        fixture.topology.clone(),
    );
    let consumer = <NatsConsumerFactory as MessageConsumerFactory<CommandAddress>>::create(
        &factory,
        provisioned.config,
    )
    .expect("create command consumer");
    let task_handler = handler.clone();
    let consumer_task = tokio::spawn(async move { consumer.run(task_handler).await });

    let publisher = fixture.connection.publisher(fixture.topology.clone());
    let quarantine_id = publish_disposition_commands(&publisher, &address)
        .await
        .expect("publish disposition commands");

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let acknowledged = handler.acknowledged.load(Ordering::Relaxed);
            let quarantined = stream_message_count(
                fixture.connection.jetstream(),
                fixture.topology.quarantine_stream(),
            )
            .await?;
            let pending_source = stream_message_count(
                fixture.connection.jetstream(),
                fixture.topology.command_stream(),
            )
            .await?;
            if acknowledged == 2 && quarantined == 1 && pending_source == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        TestResult::Ok(())
    })
    .await
    .expect("disposition observation timeout")
    .expect("disposition observation polling");

    let (_, routed) = address.as_str().split_once('.').expect("routed address");
    let quarantine_subject = format!("{}.quarantine.{routed}", address.application());
    let quarantine_stream = fixture
        .connection
        .jetstream()
        .get_stream(fixture.topology.quarantine_stream().as_str())
        .await
        .expect("quarantine stream");
    let stored = quarantine_stream
        .get_last_raw_message_by_subject(&quarantine_subject)
        .await
        .expect("quarantine message");
    let record: QuarantineRecord =
        serde_json::from_slice(&stored.payload).expect("quarantine record");
    assert_eq!(record.message_id(), quarantine_id.as_str());
    assert_eq!(record.address(), address.as_str());
    assert_eq!(
        record.correlation_id().map(CorrelationId::as_str),
        Some("quarantine-correlation")
    );
    assert_eq!(
        record.metadata().get("x-quarantine-test"),
        Some("preserved")
    );
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(record.payload_base64())
            .expect("quarantine base64 payload"),
        br#"{"kind":"quarantine"}"#
    );
    assert_eq!(record.reason(), "test quarantine");
    assert_eq!(record.attempt(), 1);
    assert!(record.source_sequence() > 0);

    consumer_task.abort();
    let _ = consumer_task.await;
    fixture.cleanup().await.expect("messaging fixture cleanup");
}

struct RoundTripHandler {
    conflict: QueryErrorPayload,
}

impl RoundTripHandler {
    fn new() -> TestResult<Self> {
        Ok(Self {
            conflict: QueryErrorPayload::new(
                QueryErrorClassification::Conflict,
                ApplicationErrorCode::new("test.conflict")?,
                "test conflict",
            )?,
        })
    }
}

struct QueryServerSetup {
    server: query::NatsQueryServer,
    invalid_scope: Result<(), QueryServerError>,
}

async fn query_server_setup(fixture: &Fixture) -> TestResult<QueryServerSetup> {
    let server = fixture.connection.query_server(
        fixture.context.application(),
        NatsQueryServerConfig::default(),
    )?;
    let invalid_scope = <query::NatsQueryServer as QueryServer<Value, Value>>::run(
        &server,
        QueryAddress::new("other", "messaging", "roundtrip")?,
        Arc::new(RoundTripHandler::new()?),
    )
    .await;
    Ok(QueryServerSetup {
        server,
        invalid_scope,
    })
}

async fn publish_malformed_query(fixture: &Fixture, address: &QueryAddress) -> TestResult<()> {
    fixture
        .connection
        .client()
        .publish(address.as_str().to_owned(), b"not-json".to_vec().into())
        .await?;
    Ok(())
}

#[async_trait]
impl QueryHandler<Value, Value> for RoundTripHandler {
    async fn handle(&self, request: QueryRequest<Value>) -> Result<Value, QueryErrorPayload> {
        if request.payload().get("error").is_some() {
            return Err(self.conflict.clone());
        }
        Ok(json!({"echo": request.payload()}))
    }
}

#[tokio::test]
async fn core_nats_query_roundtrip_preserves_errors_and_does_not_touch_jetstream() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await.expect("messaging fixture");
    let before = fixture
        .message_counts()
        .await
        .expect("message counts before query roundtrip");
    let address = fixture
        .query_address("roundtrip")
        .expect("roundtrip query address");
    let query_server = query_server_setup(&fixture)
        .await
        .expect("query server setup and invalid-scope observation");
    let invalid_server_scope = query_server
        .invalid_scope
        .expect_err("query server must reject another application");
    assert_eq!(
        invalid_server_scope.kind(),
        QueryServerErrorKind::InvalidConfiguration
    );
    let server = query_server.server;
    let server_address = address.clone();
    let round_trip_handler = Arc::new(RoundTripHandler::new().expect("roundtrip query handler"));
    let server_task = tokio::spawn(async move {
        <query::NatsQueryServer as QueryServer<Value, Value>>::run(
            &server,
            server_address,
            round_trip_handler,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A malformed request is isolated to this delivery and cannot end the server loop.
    publish_malformed_query(&fixture, &address)
        .await
        .expect("publish malformed query");
    let requester = fixture
        .connection
        .query_requester(fixture.context.application());
    let invalid_request_scope =
        <query::NatsQueryRequester as QueryRequester<Value, Value>>::request(
            &requester,
            &QueryAddress::new("other", "messaging", "roundtrip").expect("other query address"),
            query_request(
                message_id("cross-application-query").expect("cross-application query id"),
                json!({}),
            )
            .expect("cross-application query request"),
            QueryOptions::new(Duration::from_secs(3), 64 * 1024).expect("query options"),
        )
        .await
        .expect_err("query requester must reject another application");
    assert_eq!(
        invalid_request_scope.kind(),
        QueryRequestErrorKind::Rejected
    );
    let success = requester
        .request(
            &address,
            query_request(
                message_id("query-success").expect("successful query id"),
                json!({"value": 42}),
            )
            .expect("successful query request"),
            QueryOptions::new(Duration::from_secs(3), 64 * 1024).expect("query options"),
        )
        .await
        .expect("query success");
    assert_eq!(
        success.outcome(),
        &QueryOutcome::Success(json!({"echo": {"value": 42}}))
    );

    let failure: QueryResponse<Value> = requester
        .request(
            &address,
            query_request(
                message_id("query-error").expect("error query id"),
                json!({"error": true}),
            )
            .expect("error query request"),
            QueryOptions::new(Duration::from_secs(3), 64 * 1024).expect("query options"),
        )
        .await
        .expect("query application error");
    let QueryOutcome::Error(error) = failure.outcome() else {
        panic!("expected application query error");
    };
    assert_eq!(error.classification(), QueryErrorClassification::Conflict);
    assert_eq!(error.code().as_str(), "test.conflict");
    assert_eq!(
        fixture
            .message_counts()
            .await
            .expect("message counts after query roundtrip"),
        before
    );

    server_task.abort();
    let _ = server_task.await;
    fixture.cleanup().await.expect("messaging fixture cleanup");
}

struct QueueHandler {
    label: &'static str,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl QueryHandler<Value, Value> for QueueHandler {
    async fn handle(&self, _request: QueryRequest<Value>) -> Result<Value, QueryErrorPayload> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(json!({"server": self.label}))
    }
}

#[tokio::test]
async fn query_servers_in_one_queue_group_share_requests() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await.expect("messaging fixture");
    let address = fixture.query_address("queue").expect("queue query address");
    let queue_group = QueueGroup::new(format!(
        "queue-{}",
        FIXTURE_SEQUENCE.load(Ordering::Relaxed)
    ))
    .expect("queue group");
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for (label, calls) in [
        ("first", first_calls.clone()),
        ("second", second_calls.clone()),
    ] {
        let server = fixture
            .connection
            .query_server(
                fixture.context.application(),
                NatsQueryServerConfig::default().with_queue_group(queue_group.clone()),
            )
            .expect("queue query server");
        let server_address = address.clone();
        tasks.push(tokio::spawn(async move {
            <query::NatsQueryServer as QueryServer<Value, Value>>::run(
                &server,
                server_address,
                Arc::new(QueueHandler { label, calls }),
            )
            .await
        }));
    }
    tokio::time::sleep(Duration::from_millis(150)).await;

    let requester = fixture
        .connection
        .query_requester(fixture.context.application());
    for index in 0..20 {
        let _: QueryResponse<Value> = requester
            .request(
                &address,
                query_request(
                    message_id("queue-query").expect("queue query id"),
                    json!({"index": index}),
                )
                .expect("queue query request"),
                QueryOptions::new(Duration::from_secs(3), 64 * 1024).expect("query options"),
            )
            .await
            .expect("queue query response");
    }
    let total_calls = checked_add_usize(
        first_calls.load(Ordering::Relaxed),
        second_calls.load(Ordering::Relaxed),
        "combined queue query handler call count",
    )
    .expect("combined queue query handler call count arithmetic");
    assert_eq!(total_calls, 20);
    assert!(first_calls.load(Ordering::Relaxed) > 0);
    assert!(second_calls.load(Ordering::Relaxed) > 0);

    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    fixture.cleanup().await.expect("messaging fixture cleanup");
}

#[test]
fn query_control_headers_are_distinct_adapter_owned_names() {
    assert_ne!(REQUEST_ID_HEADER, CORRELATION_ID_HEADER);
}
