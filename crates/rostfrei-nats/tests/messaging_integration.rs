#![allow(dead_code)]

#[path = "../src/connection.rs"]
mod connection;
#[path = "../src/consumer.rs"]
mod consumer;
#[path = "../src/error.rs"]
mod error;
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
    collections::HashMap,
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
    CommandPublisher, ConsumerConfig, CorrelationId, DeliveryDisposition, EnvelopeContext,
    MessageConsumerFactory, MessageDelivery, MessageHandler, MessageId, MessageTimestamp,
    OutboundMessage, QuarantineReason, QueryAddress, QueryErrorClassification, QueryErrorPayload,
    QueryHandler, QueryOptions, QueryOutcome, QueryRequest, QueryRequestErrorKind, QueryRequester,
    QueryResponse, QueryServer, QueryServerErrorKind, RetryDelay, SchemaVersion, TraceContext,
};
use serde_json::{Value, json};

use connection::{NatsConnection, connect};
use consumer::{NatsConsumerFactory, QuarantineRecord};
use messaging_config::{MessagingTopology, NatsConnectionConfig, QueueGroup, StreamName};
use provisioning::{
    ApplicationMessagingConfig, provision_application_messaging, provision_durable_consumer,
};
use publish::{CONTENT_TYPE_HEADER, JSON_CONTENT_TYPE, NatsPublisher};
use query::{CORRELATION_ID_HEADER, NatsQueryServerConfig, REQUEST_ID_HEADER};

const TEST_NATS_URL_ENV: &str = "ROSTFREI_NATS_URL";
const TRACE_PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct Fixture {
    connection: NatsConnection,
    topology: MessagingTopology,
    context: BoundedContext,
}

impl Fixture {
    async fn new(url: String) -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let application = ApplicationName::new(format!("test-{}-{sequence}", std::process::id()))
            .expect("application name");
        let context = application
            .bounded_context("messaging")
            .expect("bounded context");
        let messaging = ApplicationMessagingConfig::new(&application)
            .expect("messaging config")
            .with_max_bytes(64 * 1024 * 1024)
            .expect("test stream capacity");
        let topology = messaging.topology().clone();
        let connection = connect(&NatsConnectionConfig::new(
            format!("messaging-test-{sequence}"),
            url,
        ))
        .await
        .expect("connect to test NATS");

        provision_application_messaging(connection.jetstream(), &messaging)
            .await
            .expect("provision application messaging");
        provision_application_messaging(connection.jetstream(), &messaging)
            .await
            .expect("repeated provisioning must be idempotent");
        connection
            .verify_application_messaging(&messaging)
            .await
            .expect("provisioned messaging policy");

        Self {
            connection,
            topology,
            context,
        }
    }

    fn command_address(&self, name: &str) -> CommandAddress {
        self.context.command_address(name).expect("command address")
    }

    fn query_address(&self, name: &str) -> QueryAddress {
        self.context.query_address(name).expect("query address")
    }

    async fn message_counts(&self) -> [u64; 3] {
        [
            stream_message_count(self.connection.jetstream(), self.topology.command_stream()).await,
            stream_message_count(
                self.connection.jetstream(),
                self.topology.integration_event_stream(),
            )
            .await,
            stream_message_count(
                self.connection.jetstream(),
                self.topology.quarantine_stream(),
            )
            .await,
        ]
    }

    async fn cleanup(self) {
        for stream in [
            self.topology.command_stream(),
            self.topology.integration_event_stream(),
            self.topology.quarantine_stream(),
        ] {
            self.connection
                .jetstream()
                .delete_stream(stream.as_str())
                .await
                .expect("delete test stream");
        }
        self.connection
            .drain()
            .await
            .expect("drain test connection");
    }
}

fn test_url() -> Option<String> {
    std::env::var(TEST_NATS_URL_ENV).ok()
}

fn message_id(prefix: &str) -> MessageId {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    MessageId::new(format!("{prefix}-{nanos}")).expect("message id")
}

fn query_request(id: MessageId, payload: Value) -> QueryRequest<Value> {
    QueryRequest::new(
        EnvelopeContext::new(
            id,
            SchemaVersion::new(1).expect("schema version"),
            CorrelationId::new("test-correlation").expect("correlation id"),
            None,
        ),
        MessageTimestamp::from_unix_milliseconds(1_700_000_000_000).expect("request timestamp"),
        CallerMetadata::new(),
        Some(TraceContext::new(TRACE_PARENT).expect("trace context")),
        payload,
    )
    .expect("query request")
}

async fn stream_message_count(context: &async_nats::jetstream::Context, name: &StreamName) -> u64 {
    let mut stream = context.get_stream(name.as_str()).await.expect("get stream");
    stream.info().await.expect("stream info").state.messages
}

async fn assert_application_scope_guards(fixture: &Fixture, publisher: &NatsPublisher) {
    let mismatched_policy = ApplicationMessagingConfig::new(fixture.context.application())
        .expect("mismatched messaging config")
        .with_max_bytes(32 * 1024 * 1024)
        .expect("mismatched stream capacity");
    assert!(matches!(
        fixture
            .connection
            .verify_application_messaging(&mismatched_policy)
            .await,
        Err(error::NatsError::Configuration)
    ));

    let cross_application = OutboundMessage::new(
        CommandAddress::new("other", "messaging", "publish").expect("other address"),
        message_id("cross-application"),
        br#"{"ok":true}"#.to_vec(),
    )
    .expect("cross-application message");
    assert!(matches!(
        publisher
            .publish_command_with_ack(cross_application, Duration::from_secs(5))
            .await,
        Err(error::NatsError::InvalidMessage)
    ));
}

#[tokio::test]
async fn puback_confirms_stream_sequence_duplicate_and_owned_headers() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await;
    let address = fixture.command_address("publish");
    let mut metadata = CallerMetadata::new();
    metadata
        .insert("x-test-metadata", "caller-value")
        .expect("safe metadata");
    assert!(metadata.insert("Nats-Msg-Id", "override").is_err());
    let id = message_id("publish");
    let message = OutboundMessage::new(address.clone(), id.clone(), br#"{"ok":true}"#.to_vec())
        .expect("outbound message")
        .with_metadata(metadata)
        .with_trace_context(TraceContext::new(TRACE_PARENT).expect("trace context"));
    let publisher = NatsPublisher::new(
        fixture.connection.jetstream().clone(),
        fixture.topology.clone(),
    );
    assert_application_scope_guards(&fixture, &publisher).await;

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

    fixture.cleanup().await;
}

struct DispositionHandler {
    deliveries: Mutex<HashMap<String, usize>>,
    acknowledged: AtomicUsize,
}

#[async_trait]
impl MessageHandler<CommandAddress> for DispositionHandler {
    async fn handle(&self, delivery: MessageDelivery<CommandAddress>) -> DeliveryDisposition {
        let id = delivery.message_id().as_str().to_owned();
        let mut deliveries = self.deliveries.lock().expect("delivery lock");
        let count = deliveries.entry(id.clone()).or_default();
        *count += 1;
        match id.as_str() {
            value if value.starts_with("ack-") => {
                self.acknowledged.fetch_add(1, Ordering::Relaxed);
                DeliveryDisposition::Acknowledge
            }
            value if value.starts_with("retry-") && *count == 1 => DeliveryDisposition::RetryAfter(
                RetryDelay::new(Duration::from_millis(100)).expect("retry delay"),
            ),
            value if value.starts_with("retry-") => {
                self.acknowledged.fetch_add(1, Ordering::Relaxed);
                DeliveryDisposition::Acknowledge
            }
            _ => DeliveryDisposition::Quarantine(
                QuarantineReason::new("test quarantine").expect("quarantine reason"),
            ),
        }
    }
}

async fn publish_disposition_commands(
    publisher: &NatsPublisher,
    address: &CommandAddress,
) -> MessageId {
    let quarantine_id = message_id("quarantine");
    for (id, prefix) in [
        (message_id("ack"), "ack"),
        (message_id("retry"), "retry"),
        (quarantine_id.clone(), "quarantine"),
    ] {
        let mut metadata = CallerMetadata::new();
        if prefix == "quarantine" {
            metadata
                .insert("x-quarantine-test", "preserved")
                .expect("quarantine metadata");
        }
        publisher
            .publish_command(
                OutboundMessage::new(
                    address.clone(),
                    id,
                    format!(r#"{{"kind":"{prefix}"}}"#).into_bytes(),
                )
                .expect("outbound command")
                .with_metadata(metadata),
            )
            .await
            .expect("publish command");
    }
    quarantine_id
}

#[tokio::test]
async fn durable_consumer_applies_ack_retry_and_puback_before_quarantine_term() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await;
    let address = fixture.command_address("consume");
    let name = fixture
        .context
        .consumer_name("consume", 1)
        .expect("consumer name");
    let durable = fixture
        .context
        .durable_name("consume", 1)
        .expect("durable name");
    let config = ConsumerConfig::new(
        name,
        durable,
        address.clone(),
        Duration::from_secs(10),
        Duration::from_secs(5),
        4,
        3,
    )
    .expect("consumer config");
    let provisioned =
        provision_durable_consumer(fixture.connection.jetstream(), &fixture.topology, &config)
            .await
            .expect("provision durable consumer");
    assert_eq!(provisioned.config.ack_wait, Duration::from_secs(10));
    provision_durable_consumer(fixture.connection.jetstream(), &fixture.topology, &config)
        .await
        .expect("repeated durable provisioning must be idempotent");

    let handler = Arc::new(DispositionHandler {
        deliveries: Mutex::new(HashMap::new()),
        acknowledged: AtomicUsize::new(0),
    });
    let factory = NatsConsumerFactory::new(
        fixture.connection.jetstream().clone(),
        fixture.topology.clone(),
    );
    let consumer =
        <NatsConsumerFactory as MessageConsumerFactory<CommandAddress>>::create(&factory, config)
            .expect("create command consumer");
    let task_handler = handler.clone();
    let consumer_task = tokio::spawn(async move { consumer.run(task_handler).await });

    let publisher = fixture.connection.publisher(fixture.topology.clone());
    let quarantine_id = publish_disposition_commands(&publisher, &address).await;

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let acknowledged = handler.acknowledged.load(Ordering::Relaxed);
            let quarantined = stream_message_count(
                fixture.connection.jetstream(),
                fixture.topology.quarantine_stream(),
            )
            .await;
            let pending_source = stream_message_count(
                fixture.connection.jetstream(),
                fixture.topology.command_stream(),
            )
            .await;
            if acknowledged == 2 && quarantined == 1 && pending_source == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("consumer dispositions completed");

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
    fixture.cleanup().await;
}

struct RoundTripHandler;

#[async_trait]
impl QueryHandler<Value, Value> for RoundTripHandler {
    async fn handle(&self, request: QueryRequest<Value>) -> Result<Value, QueryErrorPayload> {
        if request.payload().get("error").is_some() {
            return Err(QueryErrorPayload::new(
                QueryErrorClassification::Conflict,
                ApplicationErrorCode::new("test.conflict").expect("application code"),
                "test conflict",
            )
            .expect("query error"));
        }
        Ok(json!({"echo": request.payload()}))
    }
}

#[tokio::test]
async fn core_nats_query_roundtrip_preserves_errors_and_does_not_touch_jetstream() {
    let Some(url) = test_url() else {
        return;
    };
    let fixture = Fixture::new(url).await;
    let before = fixture.message_counts().await;
    let address = fixture.query_address("roundtrip");
    let server = fixture
        .connection
        .query_server(
            fixture.context.application(),
            NatsQueryServerConfig::default(),
        )
        .expect("query server");
    let invalid_server_scope = <query::NatsQueryServer as QueryServer<Value, Value>>::run(
        &server,
        QueryAddress::new("other", "messaging", "roundtrip").expect("other query address"),
        Arc::new(RoundTripHandler),
    )
    .await
    .expect_err("query server must reject another application");
    assert_eq!(
        invalid_server_scope.kind(),
        QueryServerErrorKind::InvalidConfiguration
    );
    let server_address = address.clone();
    let server_task = tokio::spawn(async move {
        <query::NatsQueryServer as QueryServer<Value, Value>>::run(
            &server,
            server_address,
            Arc::new(RoundTripHandler),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // A malformed request is isolated to this delivery and cannot end the server loop.
    fixture
        .connection
        .client()
        .publish(address.as_str().to_owned(), b"not-json".to_vec().into())
        .await
        .expect("publish malformed query");
    let requester = fixture
        .connection
        .query_requester(fixture.context.application());
    let invalid_request_scope =
        <query::NatsQueryRequester as QueryRequester<Value, Value>>::request(
            &requester,
            &QueryAddress::new("other", "messaging", "roundtrip").expect("other query address"),
            query_request(message_id("cross-application-query"), json!({})),
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
            query_request(message_id("query-success"), json!({"value": 42})),
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
            query_request(message_id("query-error"), json!({"error": true})),
            QueryOptions::new(Duration::from_secs(3), 64 * 1024).expect("query options"),
        )
        .await
        .expect("query application error");
    let QueryOutcome::Error(error) = failure.outcome() else {
        panic!("expected application query error");
    };
    assert_eq!(error.classification(), QueryErrorClassification::Conflict);
    assert_eq!(error.code().as_str(), "test.conflict");
    assert_eq!(fixture.message_counts().await, before);

    server_task.abort();
    let _ = server_task.await;
    fixture.cleanup().await;
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
    let fixture = Fixture::new(url).await;
    let address = fixture.query_address("queue");
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
                query_request(message_id("queue-query"), json!({"index": index})),
                QueryOptions::new(Duration::from_secs(3), 64 * 1024).expect("query options"),
            )
            .await
            .expect("queue query response");
    }
    assert_eq!(
        first_calls.load(Ordering::Relaxed) + second_calls.load(Ordering::Relaxed),
        20
    );
    assert!(first_calls.load(Ordering::Relaxed) > 0);
    assert!(second_calls.load(Ordering::Relaxed) > 0);

    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    fixture.cleanup().await;
}

#[test]
fn query_control_headers_are_distinct_adapter_owned_names() {
    assert_ne!(REQUEST_ID_HEADER, CORRELATION_ID_HEADER);
}
