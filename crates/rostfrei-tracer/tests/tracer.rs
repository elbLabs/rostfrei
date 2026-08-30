#![allow(dead_code)]

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
#[cfg(feature = "http")]
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
#[cfg(feature = "http")]
use http_body_util::BodyExt as _;
use rostfrei_core::{
    Aggregate, AggregateInstance, CommandHandler, Event, EventCodecError, EventCodecErrorKind,
    EventHistory, EventStoreError, EventStoreErrorKind, InMemoryEventStore, RecordedEvent,
    StreamId,
};
use rostfrei_registry::{CommandDefinition, DomainModule, DomainRegistry, ModuleDescriptor};
use rostfrei_tracer::{
    command_execution_fingerprint, CommandInvocation, CommandPublication, CommandReceipt,
    CommandRejection, CommandTransport, CommandTransportError, CommandTransportErrorKind,
    CommandTransportObserver, CorrelationError, CorrelationEventKind, DiscoveryError,
    ExposeTracePayloadsForLocalDevelopment, IntegrationEventObservation, OperationMode,
    RuntimeRegistrationError, SimulationRequest, SubmissionError, SubscriptionError,
    TestDefinition, TestDefinitionCollection, TestDefinitionRevision, TestReportStatus,
    TestRepository, TestRepositoryError, TestScenarioReset, TestScenarioResetError,
    TracePayloadPolicy, Tracer, TracerBuilder,
};
#[cfg(feature = "http")]
use rostfrei_tracer::{
    http::{self, HttpConfig},
    MAX_COMMAND_PAYLOAD_LEN,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{Mutex, Notify};
#[cfg(feature = "http")]
use tower::ServiceExt as _;

const AGGREGATE_TYPE: &str = "test-context/test-aggregate";
const OTHER_AGGREGATE_TYPE: &str = "test-context/other-aggregate";
const COMMAND_NAME: &str = "test-command";
#[cfg(feature = "http")]
const API_TOKEN: &str = "integration-test-capability";
#[cfg(feature = "http")]
const SIMULATION_PATH: &str =
    "/contexts/test-context/aggregates/test-aggregate/aggregate-1/commands/test-command/simulate";

#[derive(domain::BoundedContext)]
#[domain(id = "test-context", label = "Test context")]
struct TestContext;

#[derive(domain::DomainIdentity)]
#[domain(owner = TestRoot)]
struct TestRootId(String);

#[derive(domain::Entity)]
#[domain(id = "test-root", label = "Test root", owner = TestAggregate)]
struct TestRoot {
    #[domain(identity)]
    id: TestRootId,
}

#[derive(domain::Aggregate)]
#[domain(
    id = "test-aggregate",
    label = "Test aggregate",
    context = TestContext,
    root = TestRoot
)]
struct TestAggregate;

impl Aggregate for TestAggregate {
    type State = ();
    type Event = TestEvent;

    const AGGREGATE_TYPE: &'static str = AGGREGATE_TYPE;

    fn initial(_stream_id: &StreamId) -> Self::State {}

    fn apply(_state: &mut Self::State, _event: &Self::Event) {}
}

#[derive(Deserialize, Serialize)]
struct TestEvent {
    sensitive: String,
}

impl Event for TestEvent {
    fn event_type(&self) -> &'static str {
        "test-event"
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn encode_json(&self) -> Result<Vec<u8>, EventCodecError> {
        serde_json::to_vec(self).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::EncodingFailed, error.to_string())
        })
    }

    fn decode_json(event: &RecordedEvent) -> Result<Self, EventCodecError> {
        if event.event_type() != "test-event" {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnknownEventType,
                "unknown test event",
            ));
        }
        if event.schema_version() != 1 {
            return Err(EventCodecError::new(
                EventCodecErrorKind::UnsupportedSchemaVersion,
                "test events support schema version 1",
            ));
        }
        serde_json::from_slice(event.payload()).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::MalformedPayload, error.to_string())
        })
    }
}

#[derive(domain::Command)]
#[domain(
    id = "test-command",
    label = "Test command",
    owner = TestAggregate,
    rejection = TestRejection,
    json
)]
struct TestCommand {
    reject: bool,
    panic: Option<bool>,
}

impl CommandDefinition for TestCommand {
    type Aggregate = TestAggregate;

    const COMMAND_NAME: &'static str = COMMAND_NAME;
    const SCHEMA_VERSION: u32 = 1;
}

#[derive(domain::DomainError)]
#[domain(
    id = "test-rejection",
    label = "Test rejection",
    owner = TestAggregate,
    code = "TEST_REJECTION",
    message = "The test command was rejected.",
    json
)]
struct TestRejection;

impl CommandHandler<TestCommand> for TestAggregate {
    type Rejection = TestRejection;

    fn handle(
        command: &TestCommand,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        assert!(
            command.panic != Some(true),
            "deliberate command handler panic"
        );
        if command.reject {
            return Err(TestRejection);
        }
        aggregate.raise(TestEvent {
            sensitive: "accepted outcome details".to_owned(),
        });
        Ok(())
    }
}

#[derive(domain::DomainIdentity)]
#[domain(owner = OtherTestRoot)]
struct OtherTestRootId(String);

#[derive(domain::Entity)]
#[domain(
    id = "other-test-root",
    label = "Other test root",
    owner = OtherTestAggregate
)]
struct OtherTestRoot {
    #[domain(identity)]
    id: OtherTestRootId,
}

#[derive(domain::Aggregate)]
#[domain(
    id = "other-aggregate",
    label = "Other aggregate",
    context = TestContext,
    root = OtherTestRoot
)]
struct OtherTestAggregate;

impl Aggregate for OtherTestAggregate {
    type State = ();
    type Event = TestEvent;

    const AGGREGATE_TYPE: &'static str = OTHER_AGGREGATE_TYPE;

    fn initial(_stream_id: &StreamId) -> Self::State {}

    fn apply(_state: &mut Self::State, _event: &Self::Event) {}
}

#[derive(domain::Command)]
#[domain(
    id = "test-command",
    label = "Test command",
    owner = OtherTestAggregate,
    json
)]
struct OtherTestCommand {
    reject: bool,
}

impl CommandDefinition for OtherTestCommand {
    type Aggregate = OtherTestAggregate;

    const COMMAND_NAME: &'static str = COMMAND_NAME;
    const SCHEMA_VERSION: u32 = 1;
}

impl CommandHandler<OtherTestCommand> for OtherTestAggregate {
    type Rejection = TestRejection;

    fn handle(
        command: &OtherTestCommand,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        if command.reject {
            return Err(TestRejection);
        }
        aggregate.raise(TestEvent {
            sensitive: "other accepted outcome details".to_owned(),
        });
        Ok(())
    }
}

struct TestDomainModule;

impl DomainModule for TestDomainModule {
    const MODULE_NAME: &'static str = "test-domain";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![TestCommand::descriptor()],
        }
    }
}

struct OtherTestDomainModule;

impl DomainModule for OtherTestDomainModule {
    const MODULE_NAME: &'static str = "other-test-domain";

    fn descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![OtherTestCommand::descriptor()],
        }
    }
}

fn builder(history: Arc<dyn EventHistory>) -> TracerBuilder {
    let mut registry = DomainRegistry::new();
    registry.register_module::<TestDomainModule>().unwrap();
    TracerBuilder::new(history, registry)
}

fn tracer(maximum_operations: usize) -> Tracer {
    let mut builder =
        builder(Arc::new(InMemoryEventStore::new())).with_maximum_operations(maximum_operations);
    builder.register_json::<TestCommand>().unwrap();
    builder.build().unwrap()
}

async fn submit(tracer: &Tracer, operation_id: &str, payload: Value) {
    tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload,
            },
            Some(operation_id),
        )
        .await
        .unwrap();
}

async fn terminal_operation(tracer: &Tracer, operation_id: &str) -> Value {
    terminal_operation_with_trace(tracer, operation_id).await.0
}

async fn terminal_operation_with_trace(tracer: &Tracer, operation_id: &str) -> (Value, String) {
    let mut subscription = tracer.subscribe(operation_id, 0).await.unwrap();
    let mut trace = String::new();
    while let Some(event) = subscription.next().await {
        trace.push_str(&serde_json::to_string(&event).unwrap());
    }
    (
        serde_json::to_value(tracer.operation(operation_id).await.unwrap()).unwrap(),
        trace,
    )
}

#[cfg(feature = "http")]
fn authorize(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header("authorization", format!("Bearer {API_TOKEN}"))
}

#[cfg(feature = "http")]
async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http_requires_a_bearer_capability_and_reports_invalid_input() {
    let app = http::router(tracer(1024), HttpConfig::new(API_TOKEN).unwrap());

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/operations/not-present")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized.headers()["www-authenticate"], "Bearer");
    assert_eq!(unauthorized.headers()["cache-control"], "private, no-store");

    let unauthorized_malformed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(SIMULATION_PATH)
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized_malformed.status(), StatusCode::UNAUTHORIZED);

    let malformed = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri(SIMULATION_PATH)
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(malformed.headers()["cache-control"], "private, no-store");
    assert_eq!(json_body(malformed).await["code"], "invalid-json");

    let accepted = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri(SIMULATION_PATH)
                .header("content-type", "application/json")
                .header("idempotency-key", "cache-control-operation")
                .body(Body::from(
                    json!({ "schemaVersion": 1, "payload": { "reject": false } }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    assert_eq!(accepted.headers()["cache-control"], "private, no-store");

    let oversized = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri(SIMULATION_PATH)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "schemaVersion": 1,
                        "payload": "x".repeat(MAX_COMMAND_PAYLOAD_LEN + 128 * 1024),
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(json_body(oversized).await["code"], "payload-too-large");
}

#[tokio::test]
async fn default_policy_redacts_results_and_terminal_operations_are_evicted() {
    let tracer = tracer(1);

    submit(&tracer, "redacted-accepted", json!({ "reject": false })).await;
    let (accepted, accepted_trace) =
        terminal_operation_with_trace(&tracer, "redacted-accepted").await;
    assert!(accepted["result"]["predictedEvents"][0]
        .get("payload")
        .is_none());
    assert!(!accepted_trace.contains("accepted outcome details"));

    submit(&tracer, "redacted-rejected", json!({ "reject": true })).await;
    assert_eq!(
        tracer.operation("redacted-accepted").await,
        Err(SubmissionError::NotFound)
    );
    let (rejected, rejected_trace) =
        terminal_operation_with_trace(&tracer, "redacted-rejected").await;
    assert_eq!(rejected["result"]["rejection"], json!({ "redacted": true }));
    assert!(!rejected_trace.contains("TEST_REJECTION"));

    submit(
        &tracer,
        "redacted-failure",
        json!({ "reject": "not-a-boolean" }),
    )
    .await;
    let (failure, failure_trace) = terminal_operation_with_trace(&tracer, "redacted-failure").await;
    assert_eq!(failure["failure"]["code"], "invalid-command-payload");
    assert_eq!(
        failure["failure"]["message"],
        "operation failure details are redacted"
    );
    assert!(!failure_trace.contains("reject must be a boolean"));
}

struct OversizedTracePayloads;

impl TracePayloadPolicy for OversizedTracePayloads {
    fn domain_event(
        &self,
        mut event: rostfrei_tracer::PredictedDomainEvent,
    ) -> rostfrei_tracer::PredictedDomainEvent {
        event.payload = Some(json!({ "value": "x".repeat(128 * 1024) }));
        event
    }

    fn rejection(&self, rejection: Value) -> Value {
        rejection
    }

    fn failure_message(&self, message: String) -> String {
        message
    }
}

#[tokio::test]
async fn exposed_operation_payloads_are_bounded_across_retained_operations() {
    let mut builder = builder(Arc::new(InMemoryEventStore::new()))
        .with_trace_payload_policy(Arc::new(OversizedTracePayloads));
    builder.register_json::<TestCommand>().unwrap();
    let tracer = builder.build().unwrap();

    submit(
        &tracer,
        "bounded-operation-payload",
        json!({ "reject": false }),
    )
    .await;
    let (operation, trace) =
        terminal_operation_with_trace(&tracer, "bounded-operation-payload").await;
    assert!(operation["result"]["predictedEvents"][0]
        .get("payload")
        .is_none());
    assert!(!trace.contains(&"x".repeat(128 * 1024)));
}

struct MismatchedTestDomainModule;

impl DomainModule for MismatchedTestDomainModule {
    const MODULE_NAME: &'static str = "mismatched-test-domain";

    fn descriptor() -> ModuleDescriptor {
        let mut command = TestCommand::descriptor();
        command.rust_command_type = "different::TestCommand";
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![command],
        }
    }
}

#[tokio::test]
async fn runtime_bindings_scope_local_command_names_to_the_aggregate() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<TestDomainModule>().unwrap();
    registry.register_module::<OtherTestDomainModule>().unwrap();
    let mut builder = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry);
    builder.register_json::<TestCommand>().unwrap();
    builder.register_json::<OtherTestCommand>().unwrap();
    let tracer = builder.build().unwrap();

    for (aggregate_type, operation_id) in [
        (AGGREGATE_TYPE, "first-aggregate-command"),
        (OTHER_AGGREGATE_TYPE, "second-aggregate-command"),
    ] {
        tracer
            .submit_simulation(
                aggregate_type,
                "aggregate-1",
                COMMAND_NAME,
                SimulationRequest {
                    schema_version: 1,
                    payload: json!({ "reject": false }),
                },
                Some(operation_id),
            )
            .await
            .unwrap();
        terminal_operation(&tracer, operation_id).await;
    }
}

#[test]
fn runtime_binding_rejects_a_registry_descriptor_for_a_different_command_contract() {
    let mut registry = DomainRegistry::new();
    registry
        .register_module::<MismatchedTestDomainModule>()
        .unwrap();
    let mut builder = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry);

    assert!(matches!(
        builder.register_json::<TestCommand>(),
        Err(RuntimeRegistrationError::DescriptorMismatch {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));
}

#[test]
fn runtime_bindings_require_exact_registry_coverage() {
    let history: Arc<dyn EventHistory> = Arc::new(InMemoryEventStore::new());
    let mut empty_registry_builder =
        TracerBuilder::new(Arc::clone(&history), DomainRegistry::new());
    assert!(matches!(
        empty_registry_builder.register_json::<TestCommand>(),
        Err(RuntimeRegistrationError::MissingDescriptor {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));

    let missing_binding = builder(Arc::clone(&history));
    assert!(matches!(
        missing_binding.build(),
        Err(RuntimeRegistrationError::MissingBinding {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));

    let mut duplicate_binding = builder(history);
    duplicate_binding.register_json::<TestCommand>().unwrap();
    assert!(matches!(
        duplicate_binding.register_json::<TestCommand>(),
        Err(RuntimeRegistrationError::DuplicateBinding {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));
}

#[test]
fn excessive_concurrency_configuration_does_not_panic() {
    let mut builder = builder(Arc::new(InMemoryEventStore::new()))
        .with_maximum_operations(usize::MAX)
        .with_maximum_concurrent_simulations(usize::MAX);
    builder.register_json::<TestCommand>().unwrap();

    builder.build().unwrap();
}

#[tokio::test]
async fn generated_operation_ids_are_distinct_and_valid() {
    let tracer = tracer(4);
    let request = || SimulationRequest {
        schema_version: 1,
        payload: json!({ "reject": false }),
    };

    let first = tracer
        .submit_simulation(AGGREGATE_TYPE, "aggregate-1", COMMAND_NAME, request(), None)
        .await
        .unwrap();
    let second = tracer
        .submit_simulation(AGGREGATE_TYPE, "aggregate-1", COMMAND_NAME, request(), None)
        .await
        .unwrap();

    assert_ne!(first.operation_id, second.operation_id);
    assert!(first.operation_id.starts_with("simulation-"));
    assert!(second.operation_id.starts_with("simulation-"));
}

struct BlockingHistory {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl EventHistory for BlockingHistory {
    async fn load(&self, _stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(Vec::new())
    }
}

fn blocking_tracer(
    maximum_operations: usize,
    maximum_concurrent_simulations: usize,
) -> (Tracer, Arc<Notify>, Arc<Notify>) {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let history: Arc<dyn EventHistory> = Arc::new(BlockingHistory {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let mut builder = builder(history)
        .with_maximum_operations(maximum_operations)
        .with_maximum_concurrent_simulations(maximum_concurrent_simulations);
    builder.register_json::<TestCommand>().unwrap();
    (builder.build().unwrap(), entered, release)
}

fn simulation_request(reject: bool) -> SimulationRequest {
    SimulationRequest {
        schema_version: 1,
        payload: json!({ "reject": reject }),
    }
}

#[tokio::test]
async fn concurrent_admission_is_bounded_before_operation_capacity() {
    let (tracer, entered, release) = blocking_tracer(4, 1);
    tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("running-operation"),
        )
        .await
        .unwrap();
    entered.notified().await;

    let repeated = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("running-operation"),
        )
        .await
        .unwrap();
    assert_eq!(repeated.operation_id, "running-operation");
    assert_eq!(
        tracer
            .submit_simulation(
                AGGREGATE_TYPE,
                "aggregate-2",
                COMMAND_NAME,
                simulation_request(false),
                Some("concurrency-rejected"),
            )
            .await,
        Err(SubmissionError::ConcurrencyExhausted)
    );

    release.notify_one();
    let _ = terminal_operation(&tracer, "running-operation").await;
}

#[tokio::test]
async fn operation_capacity_rejects_work_when_no_terminal_record_can_be_evicted() {
    let (tracer, entered, release) = blocking_tracer(1, 1);
    tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("capacity-running"),
        )
        .await
        .unwrap();
    entered.notified().await;

    assert_eq!(
        tracer
            .submit_simulation(
                AGGREGATE_TYPE,
                "aggregate-2",
                COMMAND_NAME,
                simulation_request(false),
                Some("capacity-rejected"),
            )
            .await,
        Err(SubmissionError::CapacityExhausted)
    );

    release.notify_one();
    let _ = terminal_operation(&tracer, "capacity-running").await;
}

struct FailedHistory(EventStoreErrorKind);

#[async_trait]
impl EventHistory for FailedHistory {
    async fn load(&self, _stream_id: &StreamId) -> Result<Vec<RecordedEvent>, EventStoreError> {
        Err(EventStoreError::new(self.0, "history failure details"))
    }
}

#[tokio::test]
async fn corrupt_history_and_infrastructure_failures_have_distinct_codes() {
    for (kind, expected_code) in [
        (EventStoreErrorKind::CorruptHistory, "corrupt-history"),
        (EventStoreErrorKind::Unavailable, "history-unavailable"),
    ] {
        let mut builder = builder(Arc::new(FailedHistory(kind)));
        builder.register_json::<TestCommand>().unwrap();
        let tracer = builder.build().unwrap();
        submit(
            &tracer,
            &format!("failed-history-{expected_code}"),
            json!({ "reject": false }),
        )
        .await;
        let operation =
            terminal_operation(&tracer, &format!("failed-history-{expected_code}")).await;
        assert_eq!(operation["failure"]["code"], expected_code);
    }
}

#[tokio::test]
async fn command_handler_panics_become_one_terminal_failure_and_release_admission() {
    let mut builder =
        builder(Arc::new(InMemoryEventStore::new())).with_maximum_concurrent_simulations(1);
    builder.register_json::<TestCommand>().unwrap();
    let tracer = builder.build().unwrap();
    submit(
        &tracer,
        "panicking-operation",
        json!({ "reject": false, "panic": true }),
    )
    .await;

    let operation = terminal_operation(&tracer, "panicking-operation").await;
    assert_eq!(operation["status"], "failed");
    assert_eq!(operation["failure"]["code"], "operation-panicked");
    assert_eq!(operation["latestEventId"], 3);

    submit(
        &tracer,
        "panicking-operation-2",
        json!({ "reject": false, "panic": true }),
    )
    .await;
    let second = terminal_operation(&tracer, "panicking-operation-2").await;
    assert_eq!(second["failure"]["code"], "operation-panicked");
}

#[tokio::test]
async fn future_and_terminal_operation_cursors_are_explicit() {
    let tracer = tracer(4);
    submit(&tracer, "cursor-operation", json!({ "reject": false })).await;
    let operation = terminal_operation(&tracer, "cursor-operation").await;
    let latest = operation["latestEventId"].as_u64().unwrap();

    let terminal = tracer.subscribe("cursor-operation", latest).await.unwrap();
    assert!(terminal.is_complete().await);
    assert!(matches!(
        tracer.subscribe("cursor-operation", latest + 1).await,
        Err(SubmissionError::InvalidCursor(
            SubscriptionError::FutureCursor { latest: actual }
        )) if actual == latest
    ));
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http_reports_future_sse_cursors_with_a_stable_code() {
    let tracer = tracer(4);
    submit(&tracer, "http-cursor-operation", json!({ "reject": false })).await;
    let operation = terminal_operation(&tracer, "http-cursor-operation").await;
    let latest = operation["latestEventId"].as_u64().unwrap();
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/http-cursor-operation/events")
                .header("last-event-id", (latest + 1).to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["code"], "future-cursor");

    let terminal = app
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/http-cursor-operation/events")
                .header("last-event-id", latest.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(terminal.status(), StatusCode::NO_CONTENT);
    assert_eq!(terminal.headers()["cache-control"], "private, no-store");
}

#[derive(Clone)]
struct FakeTransport {
    invocations: Arc<Mutex<Vec<CommandInvocation>>>,
    publication: Option<CommandPublication>,
    result: Result<CommandReceipt, CommandTransportError>,
}

impl FakeTransport {
    fn accepted(prefix: &str, duplicate: bool) -> Self {
        let command_message_id = format!("{prefix}-command");
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            publication: Some(CommandPublication::new(
                command_message_id.clone(),
                duplicate,
            )),
            result: Ok(CommandReceipt::accepted(
                command_message_id,
                format!("{prefix}-response"),
                duplicate,
            )),
        }
    }

    fn rejected(prefix: &str, rejection: CommandRejection) -> Self {
        let command_message_id = format!("{prefix}-command");
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            publication: Some(CommandPublication::new(command_message_id.clone(), true)),
            result: Ok(CommandReceipt::rejected(
                command_message_id,
                format!("{prefix}-response"),
                true,
                rejection,
            )),
        }
    }

    fn failed(kind: CommandTransportErrorKind) -> Self {
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            publication: None,
            result: Err(CommandTransportError::new(
                kind,
                "private transport failure",
            )),
        }
    }

    fn failed_after_publication(prefix: &str, kind: CommandTransportErrorKind) -> Self {
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            publication: Some(CommandPublication::new(format!("{prefix}-command"), false)),
            result: Err(CommandTransportError::new(
                kind,
                "private transport failure after publication",
            )),
        }
    }
}

#[async_trait]
impl CommandTransport for FakeTransport {
    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        self.invocations.lock().await.push(invocation);
        if let Some(publication) = &self.publication {
            observer.command_published(publication.clone()).await;
        }
        self.result.clone()
    }
}

struct PanickingTransport;

#[async_trait]
impl CommandTransport for PanickingTransport {
    async fn invoke(
        &self,
        _invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        observer
            .command_published(CommandPublication::new("panic-command", false))
            .await;
        panic!("deliberate transport panic after publication");
    }
}

fn transported_tracer(
    test_transport: Option<Arc<dyn CommandTransport>>,
    dispatch_transport: Option<Arc<dyn CommandTransport>>,
    expose_payloads: bool,
) -> Tracer {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut tracer_builder = builder(history);
    if let Some(transport) = test_transport {
        tracer_builder = tracer_builder
            .with_test_event_store(store)
            .with_test_transport(transport);
    }
    if let Some(transport) = dispatch_transport {
        tracer_builder = tracer_builder.with_dispatch_transport(transport);
    }
    if expose_payloads {
        tracer_builder = tracer_builder
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    }
    tracer_builder.register_json::<TestCommand>().unwrap();
    tracer_builder.build().unwrap()
}

#[tokio::test]
async fn correlation_feed_contains_command_domain_integration_and_result_events() {
    let tracer = tracer(4);
    let queued = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "reject": false }),
            },
            Some("correlated-flow"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &queued.operation_id).await;
    tracer
        .correlation_observer(OperationMode::Simulate)
        .observe_integration_event(
            &queued.correlation_id,
            IntegrationEventObservation::new("test-event-published", 1)
                .with_message_id("integration-1")
                .with_subject("test.integration.test-context.test-event-published")
                .with_payload(json!({ "public": true })),
        )
        .await
        .unwrap();
    let mut subscription = tracer
        .subscribe_correlation(&queued.correlation_id, 0)
        .await
        .unwrap();
    let command = subscription.next().await.unwrap();
    let domain_event = subscription.next().await.unwrap();
    let result = subscription.next().await.unwrap();
    let integration_event = subscription.next().await.unwrap();

    assert!(matches!(command.kind, CorrelationEventKind::Command { .. }));
    assert!(matches!(
        domain_event.kind,
        CorrelationEventKind::DomainEvent {
            ref event_type,
            payload: None,
            ..
        } if event_type == "test-event"
    ));
    assert!(matches!(
        result.kind,
        CorrelationEventKind::CommandResult { .. }
    ));
    assert!(matches!(
        integration_event.kind,
        CorrelationEventKind::IntegrationEvent {
            ref event_type,
            payload: None,
            ..
        }
            if event_type == "test-event-published"
    ));
    assert_eq!(
        tracer
            .correlation_observer(OperationMode::Simulate)
            .observe_integration_event(
                "unknown-correlation",
                IntegrationEventObservation::new("ignored", 1),
            )
            .await,
        Err(CorrelationError::NotFound)
    );
}

#[tokio::test]
async fn correlation_observer_exposes_payloads_only_when_configured() {
    let tracer = transported_tracer(None, None, true);
    let queued = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "reject": false }),
            },
            Some("visible-correlation"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &queued.operation_id).await;
    tracer
        .correlation_observer(OperationMode::Simulate)
        .observe_integration_event(
            &queued.correlation_id,
            IntegrationEventObservation::new("visible-event", 1)
                .with_payload(json!({ "visible": true })),
        )
        .await
        .unwrap();
    tracer
        .correlation_observer(OperationMode::Simulate)
        .observe_integration_event(
            &queued.correlation_id,
            IntegrationEventObservation::new("oversized-visible-event", 1)
                .with_payload(json!({ "value": "x".repeat(128 * 1024) })),
        )
        .await
        .unwrap();

    let mut subscription = tracer
        .subscribe_correlation(&queued.correlation_id, 3)
        .await
        .unwrap();
    assert!(matches!(
        subscription.next().await.unwrap().kind,
        CorrelationEventKind::IntegrationEvent {
            payload: Some(ref payload),
            ..
        } if payload == &json!({ "visible": true })
    ));
    assert!(matches!(
        subscription.next().await.unwrap().kind,
        CorrelationEventKind::IntegrationEvent {
            ref event_type,
            payload: None,
            ..
        } if event_type == "oversized-visible-event"
    ));
}

#[tokio::test]
async fn correlation_observers_reject_events_from_another_environment() {
    let tracer = transported_tracer(
        Some(Arc::new(FakeTransport::accepted("test", false))),
        Some(Arc::new(FakeTransport::accepted("dispatch", false))),
        true,
    );
    let queued = tracer
        .submit_test(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("environment-bound-correlation"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &queued.operation_id).await;

    assert!(tracer
        .correlation_observer(OperationMode::Test)
        .observe_integration_event(
            &queued.correlation_id,
            IntegrationEventObservation::new("test-event", 1),
        )
        .await
        .is_ok());
    assert!(matches!(
        tracer
            .correlation_observer(OperationMode::Dispatch)
            .observe_integration_event(
                &queued.correlation_id,
                IntegrationEventObservation::new("production-event", 1),
            )
            .await,
        Err(CorrelationError::InvalidId(_))
    ));
}

#[cfg(feature = "http")]
#[tokio::test]
async fn correlation_sse_uses_the_capability_for_its_environment() {
    const DISPATCH_TOKEN: &str = "dispatch-integration-test-capability";
    let tracer = transported_tracer(
        None,
        Some(Arc::new(FakeTransport::accepted(
            "dispatch-correlation",
            false,
        ))),
        false,
    );
    let queued = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "reject": false }),
            },
            Some("dispatch-correlation"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &queued.operation_id).await;
    let app = http::router(
        tracer,
        HttpConfig::new(API_TOKEN)
            .unwrap()
            .with_dispatch_token(DISPATCH_TOKEN)
            .unwrap(),
    );
    let path = format!("/correlations/{}/events", queued.correlation_id);

    let forbidden = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(&path)
                .header("last-event-id", u64::MAX.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let operation_path = format!("/operations/{}/events", queued.operation_id);
    let forbidden_operation = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(operation_path)
                .header("last-event-id", u64::MAX.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden_operation.status(), StatusCode::FORBIDDEN);

    let accepted = app
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {DISPATCH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert_eq!(accepted.headers()["content-type"], "text/event-stream");
    assert_eq!(accepted.headers()["cache-control"], "private, no-store");
}

#[tokio::test]
async fn test_and_dispatch_select_separate_transports_with_shared_remote_semantics() {
    let test_transport = FakeTransport::accepted("test", false);
    let test_invocations = Arc::clone(&test_transport.invocations);
    let dispatch_transport = FakeTransport::accepted("dispatch", true);
    let dispatch_invocations = Arc::clone(&dispatch_transport.invocations);
    let tracer = transported_tracer(
        Some(Arc::new(test_transport)),
        Some(Arc::new(dispatch_transport)),
        false,
    );
    let payload = json!({ "reject": false });

    let test = tracer
        .submit_test(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: payload.clone(),
            },
            Some("same-key"),
        )
        .await
        .unwrap();
    let dispatch = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: payload.clone(),
            },
            Some("same-key"),
        )
        .await
        .unwrap();

    assert_ne!(test.operation_id, dispatch.operation_id);
    assert!(test.operation_id.starts_with("test:"));
    assert!(dispatch.operation_id.starts_with("dispatch:"));
    let (test_result, test_trace) =
        terminal_operation_with_trace(&tracer, &test.operation_id).await;
    let (dispatch_result, dispatch_trace) =
        terminal_operation_with_trace(&tracer, &dispatch.operation_id).await;

    for (operation, trace, prefix, duplicate) in [
        (&test_result, &test_trace, "test", false),
        (&dispatch_result, &dispatch_trace, "dispatch", true),
    ] {
        assert_eq!(operation["result"]["decision"], "accepted");
        assert_eq!(operation["result"]["predictedEvents"], json!([]));
        assert_eq!(operation["result"]["published"], true);
        assert_eq!(
            operation["result"]["commandMessageId"],
            format!("{prefix}-command")
        );
        assert_eq!(
            operation["result"]["responseMessageId"],
            format!("{prefix}-response")
        );
        assert_eq!(operation["result"]["duplicate"], duplicate);
        assert!(operation["result"].get("baseStreamVersion").is_none());
        assert!(operation["result"].get("appended").is_none());
        assert!(trace.contains("command-published"));
        assert!(trace.contains("command-responded"));
        assert!(trace.contains("command-accepted"));
        assert!(!trace.contains("history-replayed"));
        assert!(!trace.contains("domain-event-predicted"));
        assert!(!trace.contains("domain-events-persisted"));
        assert!(
            trace.find("command-published").unwrap() < trace.find("command-responded").unwrap()
        );
    }

    let test_invocations = test_invocations.lock().await;
    let dispatch_invocations = dispatch_invocations.lock().await;
    assert_eq!(test_invocations.len(), 1);
    assert_eq!(dispatch_invocations.len(), 1);
    let expected_fingerprint =
        command_execution_fingerprint(AGGREGATE_TYPE, "aggregate-1", COMMAND_NAME, 1, &payload);
    assert_eq!(
        test_invocations[0].execution_fingerprint(),
        expected_fingerprint
    );
    assert_eq!(
        dispatch_invocations[0].execution_fingerprint(),
        expected_fingerprint
    );
    assert_eq!(
        test_invocations[0].operation_id().as_str(),
        test.operation_id
    );
    assert_eq!(
        dispatch_invocations[0].operation_id().as_str(),
        dispatch.operation_id
    );

    let version = &tracer.catalog().contexts[0].aggregates[0].commands[0].versions[0];
    assert!(version.test_href_template.is_some());
    assert!(version.dispatch_href_template.is_some());
}

#[tokio::test]
async fn transported_rejection_has_response_evidence_without_local_append_evidence() {
    let rejection = CommandRejection::new(
        "conflict",
        "TEST_REJECTION",
        "private rejection",
        Some(json!({ "reason": "private" })),
    );
    let tracer = transported_tracer(
        None,
        Some(Arc::new(FakeTransport::rejected("rejected", rejection))),
        true,
    );

    let queued = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(true),
            Some("rejected-command"),
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&tracer, &queued.operation_id).await;

    assert_eq!(operation["result"]["decision"], "rejected");
    assert_eq!(operation["result"]["published"], true);
    assert_eq!(operation["result"]["commandMessageId"], "rejected-command");
    assert_eq!(
        operation["result"]["responseMessageId"],
        "rejected-response"
    );
    assert_eq!(operation["result"]["duplicate"], true);
    assert_eq!(operation["result"]["rejection"]["code"], "TEST_REJECTION");
    assert!(operation["result"].get("baseStreamVersion").is_none());
    assert!(operation["result"].get("appended").is_none());
    assert!(trace.contains("command-published"));
    assert!(trace.contains("command-responded"));
    assert!(trace.contains("command-rejected"));
    assert!(!trace.contains("history-replayed"));
    assert!(!trace.contains("domain-events-persisted"));
}

#[tokio::test]
async fn publication_and_receipt_mismatch_is_indeterminate() {
    let transport = FakeTransport {
        invocations: Arc::new(Mutex::new(Vec::new())),
        publication: Some(CommandPublication::new("observed-command", false)),
        result: Ok(CommandReceipt::accepted(
            "receipt-command",
            "response-message",
            false,
        )),
    };
    let tracer = transported_tracer(None, Some(Arc::new(transport)), true);

    let queued = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("mismatched-receipt"),
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&tracer, &queued.operation_id).await;

    assert_eq!(
        operation["failure"]["code"],
        "invalid-command-transport-receipt"
    );
    assert_eq!(operation["status"], "indeterminate");
    assert_eq!(operation["failure"]["commandMessageId"], "observed-command");
    assert!(trace.contains("command-published"));
    assert!(!trace.contains("command-responded"));
}

#[tokio::test]
async fn command_transport_failures_have_stable_codes() {
    for (kind, expected_code) in [
        (
            CommandTransportErrorKind::InvalidRequest,
            "invalid-command-transport-request",
        ),
        (
            CommandTransportErrorKind::Rejected,
            "command-transport-rejected",
        ),
        (
            CommandTransportErrorKind::Timeout,
            "command-transport-timeout",
        ),
        (
            CommandTransportErrorKind::Unavailable,
            "command-transport-unavailable",
        ),
        (
            CommandTransportErrorKind::InvalidConfiguration,
            "command-transport-misconfigured",
        ),
        (
            CommandTransportErrorKind::InvalidResponse,
            "invalid-command-transport-response",
        ),
    ] {
        let tracer = transported_tracer(None, Some(Arc::new(FakeTransport::failed(kind))), false);
        let queued = tracer
            .submit_dispatch(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("failed-command"),
            )
            .await
            .unwrap();
        let operation = terminal_operation(&tracer, &queued.operation_id).await;

        assert_eq!(operation["failure"]["code"], expected_code);
        assert_eq!(
            operation["failure"]["message"],
            "operation failure details are redacted"
        );
    }
}

#[tokio::test]
async fn transported_commands_require_an_idempotency_key() {
    let tracer = transported_tracer(
        Some(Arc::new(FakeTransport::accepted("test", false))),
        Some(Arc::new(FakeTransport::accepted("dispatch", false))),
        false,
    );

    for result in [
        tracer
            .submit_test(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                None,
            )
            .await,
        tracer
            .submit_dispatch(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                None,
            )
            .await,
    ] {
        assert_eq!(result, Err(SubmissionError::IdempotencyKeyRequired));
    }

    assert!(tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            None,
        )
        .await
        .is_ok());
}

#[tokio::test]
async fn simulations_cannot_occupy_transported_operation_namespaces() {
    let tracer = transported_tracer(
        Some(Arc::new(FakeTransport::accepted("test", false))),
        Some(Arc::new(FakeTransport::accepted("dispatch", false))),
        false,
    );

    for operation_id in ["test:reserved", "dispatch:reserved"] {
        assert!(matches!(
            tracer
                .submit_simulation(
                    AGGREGATE_TYPE,
                    "aggregate-1",
                    COMMAND_NAME,
                    simulation_request(false),
                    Some(operation_id),
                )
                .await,
            Err(SubmissionError::InvalidOperationId(_))
        ));
    }
}

#[tokio::test]
async fn failures_after_puback_are_indeterminate_and_preserve_publication_evidence() {
    let tracer = transported_tracer(
        None,
        Some(Arc::new(FakeTransport::failed_after_publication(
            "ambiguous",
            CommandTransportErrorKind::Timeout,
        ))),
        true,
    );
    let queued = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("ambiguous-command"),
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&tracer, &queued.operation_id).await;

    assert_eq!(operation["status"], "indeterminate");
    assert_eq!(operation["failure"]["code"], "command-transport-timeout");
    assert_eq!(
        operation["failure"]["commandMessageId"],
        "ambiguous-command"
    );
    assert_eq!(operation["failure"]["duplicate"], false);
    assert!(trace.contains("command-published"));
    assert!(trace.contains("\"type\":\"indeterminate\""));
    assert!(!trace.contains("operation.failed"));

    let mut correlation = tracer
        .subscribe_correlation(&queued.correlation_id, 0)
        .await
        .unwrap();
    assert!(matches!(
        correlation.next().await.unwrap().kind,
        CorrelationEventKind::Command { .. }
    ));
    let CorrelationEventKind::CommandResult {
        outcome, result, ..
    } = correlation.next().await.unwrap().kind
    else {
        panic!("expected an indeterminate command result");
    };
    assert_eq!(
        serde_json::to_value(outcome).unwrap(),
        Value::String("indeterminate".to_owned())
    );
    assert_eq!(result.unwrap()["commandMessageId"], "ambiguous-command");
}

#[tokio::test]
async fn transport_panics_after_puback_are_indeterminate() {
    let tracer = transported_tracer(None, Some(Arc::new(PanickingTransport)), true);
    let queued = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("panicking-transport"),
        )
        .await
        .unwrap();
    let operation = terminal_operation(&tracer, &queued.operation_id).await;

    assert_eq!(operation["status"], "indeterminate");
    assert_eq!(operation["failure"]["code"], "operation-panicked");
    assert_eq!(operation["failure"]["commandMessageId"], "panic-command");
}

#[tokio::test]
async fn transported_payload_is_validated_before_publication() {
    let transport = FakeTransport::accepted("should-not-publish", false);
    let invocations = Arc::clone(&transport.invocations);
    let tracer = transported_tracer(None, Some(Arc::new(transport)), true);

    let queued = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "reject": "not-a-boolean" }),
            },
            Some("invalid-transport-payload"),
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&tracer, &queued.operation_id).await;

    assert_eq!(operation["failure"]["code"], "invalid-command-payload");
    assert!(invocations.lock().await.is_empty());
    assert!(!trace.contains("command-published"));
    assert!(!trace.contains("command-responded"));
}

#[tokio::test]
async fn unavailable_modes_require_their_complete_configuration() {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut store_only = builder(history).with_test_event_store(store);
    store_only.register_json::<TestCommand>().unwrap();
    let store_only = store_only.build().unwrap();
    assert_eq!(
        store_only
            .submit_test(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("test-without-transport"),
            )
            .await,
        Err(SubmissionError::ModeUnavailable("test"))
    );
    let version = &store_only.catalog().contexts[0].aggregates[0].commands[0].versions[0];
    assert!(version.test_href_template.is_none());
    assert!(version.dispatch_href_template.is_none());

    let transport_only = transported_tracer(
        None,
        Some(Arc::new(FakeTransport::accepted("dispatch", false))),
        false,
    );
    assert_eq!(
        transport_only
            .submit_test(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("test-without-store"),
            )
            .await,
        Err(SubmissionError::ModeUnavailable("test"))
    );

    let no_dispatch = tracer(4);
    assert_eq!(
        no_dispatch
            .submit_dispatch(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("dispatch-without-transport"),
            )
            .await,
        Err(SubmissionError::ModeUnavailable("dispatch"))
    );
}

struct NoopReset;

#[async_trait]
impl TestScenarioReset for NoopReset {
    async fn reset(&self) -> Result<(), TestScenarioResetError> {
        Ok(())
    }
}

struct FailOnceReset(AtomicBool);

#[async_trait]
impl TestScenarioReset for FailOnceReset {
    async fn reset(&self) -> Result<(), TestScenarioResetError> {
        if self.0.swap(true, Ordering::AcqRel) {
            Ok(())
        } else {
            Err(TestScenarioResetError::Failed(
                "deliberate reset failure".to_owned(),
            ))
        }
    }
}

struct BlockingReset {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl TestScenarioReset for BlockingReset {
    async fn reset(&self) -> Result<(), TestScenarioResetError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

fn resettable_tracer(reset: Arc<dyn TestScenarioReset>) -> Tracer {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_dispatch_transport(Arc::new(FakeTransport::accepted("dispatch", false)))
        .with_test_scenario_reset(reset);
    builder.register_json::<TestCommand>().unwrap();
    builder.build().unwrap()
}

#[tokio::test]
async fn reset_invalidates_test_identities_even_when_the_runtime_reset_fails() {
    for tracer in [
        resettable_tracer(Arc::new(NoopReset)),
        resettable_tracer(Arc::new(FailOnceReset(AtomicBool::new(false)))),
    ] {
        let first = tracer
            .submit_test(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("generation-key"),
            )
            .await
            .unwrap();
        terminal_operation(&tracer, &first.operation_id).await;

        let first_reset = tracer.reset_test_scenario().await;
        assert_eq!(
            tracer.operation(&first.operation_id).await,
            Err(SubmissionError::NotFound)
        );
        assert_eq!(
            tracer.correlation_mode(&first.correlation_id),
            Err(CorrelationError::NotFound)
        );

        if first_reset.is_err() {
            assert_eq!(
                tracer
                    .submit_test(
                        AGGREGATE_TYPE,
                        "aggregate-1",
                        COMMAND_NAME,
                        simulation_request(false),
                        Some("generation-key"),
                    )
                    .await,
                Err(SubmissionError::TestScenarioUnavailable)
            );
            assert_eq!(
                tracer.aggregate_instances(AGGREGATE_TYPE).await,
                Err(DiscoveryError::TestScenarioUnavailable)
            );
            tracer.reset_test_scenario().await.unwrap();
        }

        let second = tracer
            .submit_test(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("generation-key"),
            )
            .await
            .unwrap();
        assert_ne!(first.operation_id, second.operation_id);
    }
}

#[tokio::test]
async fn test_generation_is_selected_after_an_in_progress_reset() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let tracer = resettable_tracer(Arc::new(BlockingReset {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));
    let first = tracer
        .submit_test(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("reset-race-key"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &first.operation_id).await;

    let reset_tracer = tracer.clone();
    let reset = tokio::spawn(async move { reset_tracer.reset_test_scenario().await });
    entered.notified().await;
    let submit_tracer = tracer.clone();
    let submit = tokio::spawn(async move {
        submit_tracer
            .submit_test(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                Some("reset-race-key"),
            )
            .await
    });
    tokio::task::yield_now().await;
    assert!(!submit.is_finished());

    release.notify_one();
    reset.await.unwrap().unwrap();
    let second = submit.await.unwrap().unwrap();
    assert_ne!(first.operation_id, second.operation_id);
}

#[test]
fn reset_requires_test_backing_and_test_transport() {
    let reset: Arc<dyn TestScenarioReset> = Arc::new(NoopReset);
    let history: Arc<dyn EventHistory> = Arc::new(InMemoryEventStore::new());
    let mut missing_store = builder(Arc::clone(&history))
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_test_scenario_reset(Arc::clone(&reset));
    missing_store.register_json::<TestCommand>().unwrap();
    assert!(matches!(
        missing_store.build(),
        Err(RuntimeRegistrationError::ResetWithoutTestStore)
    ));

    let store = Arc::new(InMemoryEventStore::new());
    let mut missing_transport = builder(history)
        .with_test_event_store(store)
        .with_test_scenario_reset(reset);
    missing_transport.register_json::<TestCommand>().unwrap();
    assert!(matches!(
        missing_transport.build(),
        Err(RuntimeRegistrationError::ResetWithoutTestTransport)
    ));
}

#[derive(Clone)]
struct StaticTestRepository {
    definitions: BTreeMap<String, TestDefinitionRevision>,
}

impl StaticTestRepository {
    fn one(yaml: &str) -> Self {
        let definition = TestDefinition::from_yaml(yaml).unwrap();
        let revision = TestDefinitionRevision {
            revision: "test-revision".to_owned(),
            definition,
        };
        Self {
            definitions: BTreeMap::from([(revision.definition.id.clone(), revision)]),
        }
    }
}

impl TestRepository for StaticTestRepository {
    fn list(&self) -> TestDefinitionCollection {
        TestDefinitionCollection {
            items: self
                .definitions
                .values()
                .map(TestDefinitionRevision::summary)
                .collect(),
        }
    }

    fn get(&self, id: &str) -> Result<TestDefinitionRevision, TestRepositoryError> {
        self.definitions
            .get(id)
            .cloned()
            .ok_or_else(|| TestRepositoryError::NotFound(id.to_owned()))
    }
}

fn behavioral_test_yaml(outcome: &str, setup: bool, trace: &str) -> String {
    let setup = if setup {
        r"
  commands:
    - name: test-command
      schemaVersion: 1
      aggregate:
        type: test-context/test-aggregate
        id: aggregate-1
      payload:
        reject: false
"
    } else {
        ""
    };
    format!(
        r"schemaVersion: 1
id: behavioral-test
name: Behavioral test
given:
  fixture: test-fixture
{setup}when:
  command:
    name: test-command
    schemaVersion: 1
    aggregate:
      type: test-context/test-aggregate
      id: aggregate-1
    payload:
      reject: {reject}
then:
  outcome: {outcome}
  within: 2s
{trace}",
        reject = outcome != "accepted",
    )
}

fn behavioral_tracer(
    transport: Arc<dyn CommandTransport>,
    repository: Arc<dyn TestRepository>,
) -> Tracer {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(transport)
        .with_test_fixture("test-fixture", Arc::new(NoopReset))
        .with_test_repository(repository)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<TestCommand>().unwrap();
    builder.build().unwrap()
}

#[tokio::test]
async fn behavioral_test_runs_setup_and_subject_through_the_test_transport() {
    let transport = FakeTransport::accepted("behavioral", false);
    let invocations = Arc::clone(&transport.invocations);
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        &behavioral_test_yaml("accepted", true, ""),
    ));
    let tracer = behavioral_tracer(Arc::new(transport), repository);

    let report = tracer.run_test("behavioral-test").await.unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    assert_eq!(report.revision, "test-revision");
    assert_eq!(invocations.lock().await.len(), 2);
}

#[tokio::test]
async fn behavioral_test_accepts_an_expected_business_rejection() {
    let transport = FakeTransport::rejected(
        "behavioral-rejection",
        CommandRejection::new(
            "conflict",
            "TEST_REJECTION",
            "The test command was rejected.",
            None,
        ),
    );
    let yaml = behavioral_test_yaml("\n    rejected:\n      code: TEST_REJECTION", false, "");
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(&yaml));
    let tracer = behavioral_tracer(Arc::new(transport), repository);

    let report = tracer.run_test("behavioral-test").await.unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    assert_eq!(
        report.outcome,
        Some(rostfrei_tracer::CorrelationCommandOutcome::Rejected)
    );
}

#[derive(Clone)]
struct ObservableTransport {
    correlation_id: Arc<Mutex<Option<String>>>,
    invoked: Arc<Notify>,
}

#[async_trait]
impl CommandTransport for ObservableTransport {
    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        *self.correlation_id.lock().await = Some(invocation.correlation_id().to_owned());
        self.invoked.notify_one();
        observer
            .command_published(CommandPublication::new("observed-command", false))
            .await;
        Ok(CommandReceipt::accepted(
            "observed-command",
            "observed-response",
            false,
        ))
    }
}

#[tokio::test]
async fn behavioral_test_waits_for_correlated_event_expectations() {
    let correlation_id = Arc::new(Mutex::new(None));
    let invoked = Arc::new(Notify::new());
    let transport = ObservableTransport {
        correlation_id: Arc::clone(&correlation_id),
        invoked: Arc::clone(&invoked),
    };
    let trace = r"  trace:
    contains:
      - kind: integration-event
        name: test-published
        schemaVersion: 1
";
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        &behavioral_test_yaml("accepted", false, trace),
    ));
    let tracer = behavioral_tracer(Arc::new(transport), repository);
    let run_tracer = tracer.clone();
    let run = tokio::spawn(async move { run_tracer.run_test("behavioral-test").await });
    invoked.notified().await;
    let correlation_id = correlation_id.lock().await.clone().unwrap();

    tracer
        .correlation_observer(OperationMode::Test)
        .observe_integration_event(
            &correlation_id,
            IntegrationEventObservation::new("test-published", 1),
        )
        .await
        .unwrap();
    let report = run.await.unwrap().unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    assert!(report.expectations[0].matched_event_id.is_some());
}

#[cfg(feature = "http")]
#[tokio::test]
async fn behavioral_tests_are_discoverable_and_runnable_over_http() {
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        &behavioral_test_yaml("accepted", false, ""),
    ));
    let tracer = behavioral_tracer(
        Arc::new(FakeTransport::accepted("behavioral-http", false)),
        repository,
    );
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());

    let list = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/tests")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await["items"][0]["id"], "behavioral-test");

    let run = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/tests/behavioral-test/runs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(run.status(), StatusCode::OK);
    assert_eq!(run.headers()["cache-control"], "private, no-store");
    assert_eq!(json_body(run).await["status"], "passed");
}
