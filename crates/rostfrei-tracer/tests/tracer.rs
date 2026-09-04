#![allow(dead_code)]

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
    AggregateInstance, CommandHandler, EventHistory, EventStoreError, EventStoreErrorKind,
    InMemoryEventStore, RecordedEvent, StreamId,
};
use rostfrei_messaging_core::MessageSeries;
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{
    CommandInvocation, CommandPublication, CommandReceipt, CommandRejection, CommandTransport,
    CommandTransportError, CommandTransportErrorKind, CommandTransportObserver, CorrelationError,
    CorrelationEventKind, DiscoveryError, ExposeTracePayloadsForLocalDevelopment, Fixture,
    IntegrationEventObservation, MessageSeriesCaptureError, OperationMode, OperationResult,
    RuntimeRegistrationError, SimulationRequest, SubmissionError, SubscriptionError,
    TestDefinition, TestDefinitionCollection, TestDefinitionRevision, TestReportStatus,
    TestRepository, TestRepositoryError, TestScenarioReset, TestScenarioResetError,
    TracePayloadPolicy, Tracer, TracerBuilder, command_execution_fingerprint,
};
#[cfg(feature = "http")]
use rostfrei_tracer::{
    MAX_COMMAND_PAYLOAD_LEN,
    http::{self, HttpConfig},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
struct TestRootId(String);

#[derive(domain::Entity)]
#[domain(id = "test-root", label = "Test root")]
struct TestRoot {
    id: TestRootId,
}

impl domain::EntityDefinition for TestRoot {
    type Owner = TestAggregate;
    type Identity = TestRootId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(domain::Aggregate)]
#[domain(id = "test-aggregate", label = "Test aggregate")]
struct TestAggregate;

impl domain::AggregateDefinition for TestAggregate {
    type Context = TestContext;
    type Root = TestRoot;
    type Event = TestEvents;
}

#[derive(Deserialize, domain::DomainEvent, Serialize)]
#[domain(id = "test-event", label = "Test event")]
struct TestEvent {
    sensitive: String,
}

#[derive(domain::AggregateEvents)]
enum TestEvents {
    TestEvent(TestEvent),
}

impl rostfrei::Initialize<TestAggregate> for TestRoot {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: TestRootId(stream_id.aggregate_id().as_str().to_owned()),
        }
    }
}

impl rostfrei::Apply<TestEvent> for TestRoot {
    fn apply(&mut self, _event: &TestEvent) {}
}

#[derive(domain::Command)]
#[domain(id = "test-command", label = "Test command")]
struct TestCommand {
    reject: bool,
    panic: Option<bool>,
    padding: Option<String>,
}

#[derive(domain::DomainError)]
#[domain(
    id = "test-rejection",
    label = "Test rejection",
    code = "TEST_REJECTION",
    message = "The test command was rejected."
)]
struct TestRejection;

impl CommandHandler<TestCommand> for TestAggregate {
    type Rejection = TestRejection;

    #[allow(
        clippy::panic_in_result_fn,
        reason = "the panic path is the behavior exercised by operation panic tests"
    )]
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
struct OtherTestRootId(String);

#[derive(domain::Entity)]
#[domain(id = "other-test-root", label = "Other test root")]
struct OtherTestRoot {
    id: OtherTestRootId,
}

impl domain::EntityDefinition for OtherTestRoot {
    type Owner = OtherTestAggregate;
    type Identity = OtherTestRootId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(domain::Aggregate)]
#[domain(id = "other-aggregate", label = "Other aggregate")]
struct OtherTestAggregate;

impl domain::AggregateDefinition for OtherTestAggregate {
    type Context = TestContext;
    type Root = OtherTestRoot;
    type Event = TestEvents;
}

impl rostfrei::Initialize<OtherTestAggregate> for OtherTestRoot {
    fn initialize(stream_id: &StreamId) -> Self {
        Self {
            id: OtherTestRootId(stream_id.aggregate_id().as_str().to_owned()),
        }
    }
}

impl rostfrei::Apply<TestEvent> for OtherTestRoot {
    fn apply(&mut self, _event: &TestEvent) {}
}

#[derive(domain::Command)]
#[domain(id = "test-command", label = "Test command")]
struct OtherTestCommand {
    reject: bool,
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

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn builder(history: Arc<dyn EventHistory>) -> TracerBuilder {
    let mut registry = DomainRegistry::new();
    registry
        .register_command::<TestAggregate, TestCommand>()
        .unwrap();
    TracerBuilder::new(history, registry)
}

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn tracer(maximum_operations: usize) -> Tracer {
    let mut builder =
        builder(Arc::new(InMemoryEventStore::new())).with_maximum_operations(maximum_operations);
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    builder.build().unwrap()
}

#[allow(clippy::unwrap_used, reason = "test submissions must succeed")]
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

#[allow(clippy::unwrap_used, reason = "test trace collection must succeed")]
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
#[allow(clippy::unwrap_used, reason = "test response decoding must succeed")]
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

#[cfg(feature = "http")]
#[tokio::test]
async fn http_catalog_omits_dispatch_without_a_dispatch_capability() {
    let tracer = transported_tracer(
        None,
        Some(Arc::new(FakeTransport::accepted("catalog-dispatch", false))),
        false,
    );
    assert!(
        tracer.catalog().contexts[0].aggregates[0].commands[0].versions[0]
            .dispatch_href_template
            .is_some()
    );
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());

    let response = app
        .oneshot(
            authorize(Request::builder())
                .uri("/catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let catalog = json_body(response).await;
    assert!(catalog["contexts"][0]["aggregates"][0]["commands"][0]["versions"][0]
        ["dispatchHrefTemplate"]
        .is_null());
}

#[tokio::test]
#[allow(clippy::unwrap_used, reason = "test timeout fixtures must parse")]
async fn default_policy_redacts_results_and_terminal_operations_are_evicted() {
    let tracer = tracer(1);

    submit(&tracer, "redacted-accepted", json!({ "reject": false })).await;
    let (accepted, accepted_trace) =
        terminal_operation_with_trace(&tracer, "redacted-accepted").await;
    assert!(
        accepted["result"]["predictedEvents"][0]
            .get("payload")
            .is_none()
    );
    assert!(!accepted_trace.contains("accepted outcome details"));

    submit(&tracer, "redacted-rejected", json!({ "reject": true })).await;
    assert_eq!(
        tracer.operation("redacted-accepted").await,
        Err(SubmissionError::NotFound)
    );
    assert!(matches!(
        tracer
            .operation_message_series(
                "redacted-accepted",
                "1s".parse().unwrap(),
                "1ms".parse().unwrap(),
            )
            .await,
        Err(MessageSeriesCaptureError::Operation(
            SubmissionError::NotFound
        ))
    ));
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

#[tokio::test]
#[allow(
    clippy::unwrap_used,
    reason = "test operations and subscriptions must succeed"
)]
async fn active_correlation_subscribers_prevent_terminal_eviction() {
    let tracer = tracer(1);
    submit(&tracer, "retained-correlation", json!({ "reject": false })).await;
    terminal_operation(&tracer, "retained-correlation").await;
    let mut correlation = tracer
        .subscribe_correlation("retained-correlation", 0)
        .await
        .unwrap();
    while let Some(event) = correlation.next().await {
        if matches!(event.kind, CorrelationEventKind::CommandResult { .. }) {
            break;
        }
    }
    tokio::task::yield_now().await;

    let blocked = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "reject": false }),
            },
            Some("replacement-operation"),
        )
        .await;
    assert_eq!(blocked, Err(SubmissionError::CapacityExhausted));
    assert!(tracer.operation("retained-correlation").await.is_ok());

    drop(correlation);
    submit(&tracer, "replacement-operation", json!({ "reject": false })).await;
    assert_eq!(
        tracer.operation("retained-correlation").await,
        Err(SubmissionError::NotFound)
    );
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
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    let tracer = builder.build().unwrap();

    submit(
        &tracer,
        "bounded-operation-payload",
        json!({ "reject": false }),
    )
    .await;
    let (operation, trace) =
        terminal_operation_with_trace(&tracer, "bounded-operation-payload").await;
    assert!(
        operation["result"]["predictedEvents"][0]
            .get("payload")
            .is_none()
    );
    assert!(!trace.contains(&"x".repeat(128 * 1024)));
}

#[tokio::test]
async fn runtime_bindings_scope_local_command_names_to_the_aggregate() {
    let mut registry = DomainRegistry::new();
    registry
        .register_command::<TestAggregate, TestCommand>()
        .unwrap();
    registry
        .register_command::<OtherTestAggregate, OtherTestCommand>()
        .unwrap();
    let mut builder = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), registry);
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    builder
        .register_json::<OtherTestAggregate, OtherTestCommand>()
        .unwrap();
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
fn runtime_bindings_require_exact_registry_coverage() {
    let history: Arc<dyn EventHistory> = Arc::new(InMemoryEventStore::new());
    let mut empty_registry_builder =
        TracerBuilder::new(Arc::clone(&history), DomainRegistry::new());
    assert!(matches!(
        empty_registry_builder.register_json::<TestAggregate, TestCommand>(),
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
    duplicate_binding
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    assert!(matches!(
        duplicate_binding.register_json::<TestAggregate, TestCommand>(),
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
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();

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

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
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
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
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
#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
async fn simulation_admission_does_not_exhaust_dispatch_capacity() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let history: Arc<dyn EventHistory> = Arc::new(BlockingHistory {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let mut builder = builder(history)
        .with_dispatch_transport(Arc::new(FakeTransport::accepted(
            "dispatch-reserved",
            false,
        )))
        .with_maximum_operations(4)
        .with_maximum_concurrent_simulations(1);
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    let tracer = builder.build().unwrap();

    tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("blocking-simulation"),
        )
        .await
        .unwrap();
    entered.notified().await;

    let dispatch = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-2",
            COMMAND_NAME,
            simulation_request(false),
            Some("dispatch-with-reserved-capacity"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &dispatch.operation_id).await;

    release.notify_one();
    let _ = terminal_operation(&tracer, "blocking-simulation").await;
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
        builder
            .register_json::<TestAggregate, TestCommand>()
            .unwrap();
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
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
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
    #[allow(
        clippy::panic,
        reason = "the transport intentionally panics for this test"
    )]
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

struct HangingTransport;

#[async_trait]
impl CommandTransport for HangingTransport {
    async fn invoke(
        &self,
        _invocation: CommandInvocation,
        _observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        std::future::pending().await
    }
}

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
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
    tracer_builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
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
            IntegrationEventObservation::new(
                "integration-1",
                "test-event-published",
                1,
                "test.integration.test-context.test-event-published",
            )
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
                IntegrationEventObservation::new(
                    "ignored-1",
                    "ignored",
                    1,
                    "test.integration.ignored",
                ),
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
            IntegrationEventObservation::new(
                "visible-event-1",
                "visible-event",
                1,
                "test.integration.visible-event",
            )
            .with_payload(json!({ "visible": true })),
        )
        .await
        .unwrap();
    tracer
        .correlation_observer(OperationMode::Simulate)
        .observe_integration_event(
            &queued.correlation_id,
            IntegrationEventObservation::new(
                "oversized-visible-event-1",
                "oversized-visible-event",
                1,
                "test.integration.oversized-visible-event",
            )
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

    assert!(
        tracer
            .correlation_observer(OperationMode::Test)
            .observe_integration_event(
                &queued.correlation_id,
                IntegrationEventObservation::new(
                    "test-event-1",
                    "test-event",
                    1,
                    "test.integration.test-event",
                ),
            )
            .await
            .is_ok()
    );
    assert!(matches!(
        tracer
            .correlation_observer(OperationMode::Dispatch)
            .observe_integration_event(
                &queued.correlation_id,
                IntegrationEventObservation::new(
                    "production-event-1",
                    "production-event",
                    1,
                    "production.integration.production-event",
                ),
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

#[cfg(feature = "http")]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "test HTTP setup and decoding must succeed"
)]
async fn operation_message_series_uses_mode_capabilities_and_reports_fidelity() {
    const DISPATCH_TOKEN: &str = "dispatch-message-series-capability";
    let tracer = transported_tracer(
        Some(Arc::new(FakeTransport::accepted("series-test", false))),
        Some(Arc::new(FakeTransport::accepted("series-dispatch", false))),
        false,
    );
    let test = tracer
        .submit_test(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("message-series-test"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &test.operation_id).await;
    let correlated = IntegrationEventObservation::new(
        "series-integration",
        "test-event-published",
        1,
        "test.integration.test-event-published",
    )
    .with_causation_id("series-test-command")
    .with_payload(json!({ "sensitive": true }));
    let observer = tracer.correlation_observer(OperationMode::Test);
    observer
        .observe_integration_event(&test.correlation_id, correlated.clone())
        .await
        .unwrap();
    observer
        .observe_integration_event(&test.correlation_id, correlated)
        .await
        .unwrap();

    let dispatch = tracer
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("message-series-dispatch"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &dispatch.operation_id).await;
    let preview = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("message-series-preview"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &preview.operation_id).await;

    assert_eq!(
        test.message_series_href,
        format!("/operations/{}/message-series", test.operation_id)
    );
    let app = http::router(
        tracer,
        HttpConfig::new(API_TOKEN)
            .unwrap()
            .with_dispatch_token(DISPATCH_TOKEN)
            .unwrap(),
    );
    let test_path = format!("{}?within=1s&settleFor=1ms", test.message_series_href);
    let dispatch_path = format!("{}?within=1s&settleFor=1ms", dispatch.message_series_href);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&test_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let wrong_test_capability = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&test_path)
                .header("authorization", format!("Bearer {DISPATCH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_test_capability.status(), StatusCode::FORBIDDEN);

    let wrong_dispatch_capability = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(&dispatch_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_dispatch_capability.status(), StatusCode::FORBIDDEN);

    let exact = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(&test_path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(exact.status(), StatusCode::OK);
    let exact = json_body(exact).await;
    assert_eq!(exact["operationId"], test.operation_id);
    assert_eq!(exact["correlationId"], test.correlation_id);
    assert_eq!(exact["mode"], "test");
    assert_eq!(exact["capture"]["settled"], true);
    assert_eq!(exact["capture"]["settledFor"], "1ms");
    assert_eq!(exact["capture"]["fidelity"], "exact");
    assert!(exact["capture"].get("note").is_none());
    assert_eq!(
        exact["messageSeries"]["messages"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        exact["messageSeries"]["messages"][0]["messageId"],
        "series-test-command"
    );
    assert!(
        exact["messageSeries"]["messages"][0]
            .get("payload")
            .is_none()
    );
    assert_eq!(
        exact["messageSeries"]["messages"][1]["causationId"],
        "series-test-command"
    );
    assert!(
        exact["messageSeries"]["messages"][1]
            .get("payload")
            .is_none()
    );
    assert_eq!(
        exact["messageSeries"]["commandOutcomes"][0]["responseMessageId"],
        "series-test-response"
    );

    let accepted_dispatch = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(dispatch_path)
                .header("authorization", format!("Bearer {DISPATCH_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted_dispatch.status(), StatusCode::OK);
    assert_eq!(
        json_body(accepted_dispatch).await["capture"]["fidelity"],
        "exact"
    );

    let grouped = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(format!(
                    "{}?within=1s&settleFor=1ms",
                    preview.message_series_href
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(grouped.status(), StatusCode::OK);
    let grouped = json_body(grouped).await;
    assert_eq!(grouped["capture"]["fidelity"], "grouped");
    assert!(
        grouped["capture"]["note"]
            .as_str()
            .unwrap()
            .contains("synthetic")
    );
    assert_eq!(
        grouped["messageSeries"]["commandOutcomes"][0]["outcome"]["status"],
        "accepted"
    );

    let followed = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(&test.message_series_href)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(followed.status(), StatusCode::OK);
    assert_eq!(json_body(followed).await["capture"]["settledFor"], "500ms");

    let invalid_timeout = app
        .oneshot(
            authorize(Request::builder())
                .uri(format!(
                    "{}?within=61s&settleFor=1ms",
                    preview.message_series_href
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid_timeout.status(), StatusCode::BAD_REQUEST);
}

#[cfg(feature = "http")]
#[tokio::test]
#[allow(
    clippy::unwrap_used,
    reason = "test HTTP setup and decoding must succeed"
)]
async fn operation_message_series_waits_for_terminal_status_and_correlation_idle() {
    let (tracer, entered, release) = blocking_tracer(2, 1);
    let operation = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("settled-message-series"),
        )
        .await
        .unwrap();
    entered.notified().await;
    let app = http::router(tracer.clone(), HttpConfig::new(API_TOKEN).unwrap());
    let path = format!(
        "{}?within=1s&settleFor=100ms",
        operation.message_series_href
    );
    let capture = tokio::spawn(async move {
        app.oneshot(
            authorize(Request::builder())
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    });

    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    assert!(!capture.is_finished());
    release.notify_one();
    terminal_operation(&tracer, &operation.operation_id).await;
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    tracer
        .correlation_observer(OperationMode::Simulate)
        .observe_integration_event(
            &operation.correlation_id,
            IntegrationEventObservation::new(
                "late-series-event",
                "late-event",
                1,
                "test.integration.late-event",
            ),
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(70)).await;
    assert!(!capture.is_finished());

    let response = capture.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["capture"]["settled"], true);
    assert!(
        body["messageSeries"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["messageId"] == "late-series-event")
    );
}

#[cfg(feature = "http")]
#[tokio::test]
#[allow(
    clippy::unwrap_used,
    reason = "test HTTP setup and decoding must succeed"
)]
async fn operation_message_series_timeout_is_unsettled_and_does_not_cancel_execution() {
    let (tracer, entered, release) = blocking_tracer(2, 1);
    let operation = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("unsettled-message-series"),
        )
        .await
        .unwrap();
    entered.notified().await;
    let app = http::router(tracer.clone(), HttpConfig::new(API_TOKEN).unwrap());
    let response = app
        .oneshot(
            authorize(Request::builder())
                .uri(format!(
                    "{}?within=30ms&settleFor=10ms",
                    operation.message_series_href
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["capture"]["settled"], false);
    assert_eq!(body["capture"]["fidelity"], "grouped");
    assert_eq!(
        tracer
            .operation(&operation.operation_id)
            .await
            .unwrap()
            .status,
        rostfrei_tracer::OperationStatus::Running
    );

    release.notify_one();
    let completed = terminal_operation(&tracer, &operation.operation_id).await;
    assert_eq!(completed["status"], "completed");
}

#[tokio::test]
#[allow(clippy::unwrap_used, reason = "test capture setup must succeed")]
async fn active_message_series_capture_prevents_terminal_eviction() {
    let tracer = tracer(1);
    submit(&tracer, "captured-operation", json!({ "reject": false })).await;
    terminal_operation(&tracer, "captured-operation").await;
    let mut capture = Box::pin(tracer.operation_message_series(
        "captured-operation",
        "1s".parse().unwrap(),
        "200ms".parse().unwrap(),
    ));
    assert!(matches!(
        futures_util::poll!(&mut capture),
        std::task::Poll::Pending
    ));

    let blocked = tracer
        .submit_simulation(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("replacement-after-capture"),
        )
        .await;
    assert_eq!(blocked, Err(SubmissionError::CapacityExhausted));
    assert!(capture.await.unwrap().capture.settled);

    submit(
        &tracer,
        "replacement-after-capture",
        json!({ "reject": false }),
    )
    .await;
}

#[tokio::test]
#[allow(clippy::unwrap_used, reason = "test capture setup must succeed")]
async fn conflicting_duplicate_message_identity_is_grouped() {
    let tracer = transported_tracer(
        Some(Arc::new(FakeTransport::accepted("duplicate-series", false))),
        None,
        false,
    );
    let operation = tracer
        .submit_test(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            simulation_request(false),
            Some("duplicate-series"),
        )
        .await
        .unwrap();
    terminal_operation(&tracer, &operation.operation_id).await;
    let observer = tracer.correlation_observer(OperationMode::Test);
    let first = IntegrationEventObservation::new(
        "duplicate-event",
        "first-name",
        1,
        "test.integration.first-name",
    )
    .with_causation_id("duplicate-series-command");
    observer
        .observe_integration_event(&operation.correlation_id, first.clone())
        .await
        .unwrap();
    observer
        .observe_integration_event(&operation.correlation_id, first)
        .await
        .unwrap();
    assert!(
        observer
            .observe_integration_event(
                &operation.correlation_id,
                IntegrationEventObservation::new(
                    "duplicate-event",
                    "conflicting-name",
                    1,
                    "test.integration.conflicting-name",
                )
                .with_causation_id("duplicate-series-command"),
            )
            .await
            .is_err()
    );

    let capture = tracer
        .operation_message_series(
            &operation.operation_id,
            "1s".parse().unwrap(),
            "1ms".parse().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(capture).unwrap()["capture"]["fidelity"],
        "grouped"
    );
}

#[test]
fn accepted_results_serialize_predictions_only_for_simulation() {
    let simulation = serde_json::to_value(OperationResult::Accepted {
        base_stream_version: Some(0),
        predicted_events: Vec::new(),
        appended: Some(false),
        published: false,
        command_message_id: None,
        response_message_id: None,
        duplicate: None,
    })
    .unwrap();
    assert_eq!(simulation.get("predictedEvents"), Some(&json!([])));

    let transported = serde_json::to_value(OperationResult::Accepted {
        base_stream_version: None,
        predicted_events: Vec::new(),
        appended: None,
        published: true,
        command_message_id: Some("command-1".to_owned()),
        response_message_id: Some("response-1".to_owned()),
        duplicate: Some(false),
    })
    .unwrap();
    assert!(transported.get("predictedEvents").is_none());
}

#[allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    reason = "serialized operation fixtures use required fields and terminal trace events"
)]
fn assert_transported_operation(operation: &Value, trace: &str, prefix: &str, duplicate: bool) {
    assert_eq!(operation["result"]["decision"], "accepted");
    assert!(operation["result"].get("predictedEvents").is_none());
    assert_eq!(operation["result"]["published"], true);
    assert_eq!(operation["events"]["kind"], "observed");
    assert_eq!(
        operation["events"]["href"],
        operation["correlationEventsHref"]
    );
    assert_eq!(
        operation["operationEventsHref"],
        format!(
            "/operations/{}/events",
            operation["operationId"].as_str().unwrap()
        )
    );
    assert_eq!(
        operation["correlationEventsHref"],
        format!(
            "/correlations/{}/events",
            operation["correlationId"].as_str().unwrap()
        )
    );
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
    assert!(trace.find("command-published").unwrap() < trace.find("command-responded").unwrap());
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
        assert_transported_operation(operation, trace, prefix, duplicate);
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

    assert!(
        tracer
            .submit_simulation(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                simulation_request(false),
                None,
            )
            .await
            .is_ok()
    );
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
    let (outcome, result) = loop {
        let event = correlation.next().await.unwrap();
        if let CorrelationEventKind::CommandResult {
            outcome, result, ..
        } = event.kind
        {
            break (outcome, result);
        }
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
    store_only
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
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
    async fn reset(&self, _fixture: &Fixture) -> Result<(), TestScenarioResetError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingReset {
    fixture_ids: Mutex<Vec<String>>,
}

#[async_trait]
impl TestScenarioReset for RecordingReset {
    async fn reset(&self, fixture: &Fixture) -> Result<(), TestScenarioResetError> {
        self.fixture_ids.lock().await.push(fixture.id().to_owned());
        Ok(())
    }
}

struct FailOnceReset(AtomicBool);

#[async_trait]
impl TestScenarioReset for FailOnceReset {
    async fn reset(&self, _fixture: &Fixture) -> Result<(), TestScenarioResetError> {
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
    async fn reset(&self, _fixture: &Fixture) -> Result<(), TestScenarioResetError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn empty_fixture(id: &str) -> Fixture {
    Fixture::new(id, format!("{id}-revision"), MessageSeries::new()).unwrap()
}

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn resettable_tracer(reset: Arc<dyn TestScenarioReset>) -> Tracer {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_dispatch_transport(Arc::new(FakeTransport::accepted("dispatch", false)))
        .with_test_scenario_reset(reset)
        .with_default_test_fixture(empty_fixture("default-fixture"));
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    builder.build().unwrap()
}

#[tokio::test]
async fn standalone_reset_applies_the_default_fixture() {
    let reset = Arc::new(RecordingReset::default());
    let tracer = resettable_tracer(reset.clone());

    tracer.reset_test_scenario().await.unwrap();

    assert_eq!(
        reset.fixture_ids.lock().await.as_slice(),
        ["default-fixture"]
    );
}

#[tokio::test]
#[allow(
    clippy::unwrap_used,
    reason = "test operations and timeout fixtures must succeed"
)]
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
        assert!(matches!(
            tracer
                .operation_message_series(
                    &first.operation_id,
                    "1s".parse().unwrap(),
                    "1ms".parse().unwrap(),
                )
                .await,
            Err(MessageSeriesCaptureError::Operation(
                SubmissionError::NotFound
            ))
        ));

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
        .with_test_scenario_reset(Arc::clone(&reset))
        .with_default_test_fixture(empty_fixture("default-fixture"));
    missing_store
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    assert!(matches!(
        missing_store.build(),
        Err(RuntimeRegistrationError::ResetWithoutTestStore)
    ));

    let store = Arc::new(InMemoryEventStore::new());
    let mut missing_transport = builder(history)
        .with_test_event_store(store)
        .with_test_scenario_reset(reset)
        .with_default_test_fixture(empty_fixture("default-fixture"));
    missing_transport
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    assert!(matches!(
        missing_transport.build(),
        Err(RuntimeRegistrationError::ResetWithoutTestTransport)
    ));
}

#[test]
fn fixture_registry_configuration_is_validated_at_build() {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut missing_default = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_test_scenario_reset(Arc::new(NoopReset));
    missing_default
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    assert!(matches!(
        missing_default.build(),
        Err(RuntimeRegistrationError::ResetWithoutDefaultTestFixture)
    ));

    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let duplicate = empty_fixture("duplicate-fixture");
    let mut duplicate_fixture = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_test_scenario_reset(Arc::new(NoopReset))
        .with_default_test_fixture(duplicate.clone())
        .with_test_fixture(duplicate);
    duplicate_fixture
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    assert!(matches!(
        duplicate_fixture.build(),
        Err(RuntimeRegistrationError::DuplicateTestFixture { fixture_id })
            if fixture_id == "duplicate-fixture"
    ));
}

#[test]
fn catalog_lists_all_registered_fixtures_in_id_order() {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_test_scenario_reset(Arc::new(NoopReset))
        .with_default_test_fixture(empty_fixture("z-default"))
        .with_test_fixture(empty_fixture("a-additional"));
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    let tracer = builder.build().unwrap();

    assert_eq!(
        tracer.catalog().test_scenario.as_ref().unwrap().fixtures,
        ["a-additional", "z-default"]
    );
}

#[derive(Clone)]
struct StaticTestRepository {
    definitions: BTreeMap<String, TestDefinitionRevision>,
}

impl StaticTestRepository {
    #[allow(
        clippy::unwrap_used,
        reason = "the inline test definition must be valid"
    )]
    fn one(value: Value) -> Self {
        let definition = TestDefinition::from_json_value(value).unwrap();
        let revision = TestDefinitionRevision {
            revision: "test-revision".to_owned(),
            definition,
        };
        Self {
            definitions: BTreeMap::from([(revision.definition.id().to_owned(), revision)]),
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

fn behavioral_test_definition(outcome: &Value) -> Value {
    let reject = outcome != &json!("accepted");
    json!({
        "schemaVersion": 1,
        "id": "behavioral-test",
        "name": "Behavioral test",
        "setup": {
            "fixture": "test-fixture"
        },
        "expected": {
            "within": "2s",
            "settleFor": "1ms",
            "graphs": [{
                "nodes": [{
                    "kind": "command",
                    "key": "subject",
                    "name": COMMAND_NAME,
                    "schemaVersion": 1,
                    "aggregate": {
                        "type": AGGREGATE_TYPE,
                        "id": "aggregate-1"
                    },
                    "payload": { "reject": reject },
                    "outcome": outcome
                }]
            }]
        }
    })
}

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn behavioral_tracer(
    transport: Arc<dyn CommandTransport>,
    repository: Arc<dyn TestRepository>,
) -> Tracer {
    behavioral_tracer_with_reset(transport, repository, Arc::new(NoopReset))
}

#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn behavioral_tracer_with_reset(
    transport: Arc<dyn CommandTransport>,
    repository: Arc<dyn TestRepository>,
    reset: Arc<dyn TestScenarioReset>,
) -> Tracer {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(transport)
        .with_test_scenario_reset(reset)
        .with_default_test_fixture(empty_fixture("default-fixture"))
        .with_test_fixture(empty_fixture("test-fixture"))
        .with_test_repository(repository)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    builder.build().unwrap()
}

#[test]
#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
fn repository_definitions_are_validated_against_the_fixture_registry() {
    let mut definition = behavioral_test_definition(&json!("accepted"));
    definition["setup"]["fixture"] = json!("unregistered-fixture");
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(definition));
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted("test", false)))
        .with_test_scenario_reset(Arc::new(NoopReset))
        .with_default_test_fixture(empty_fixture("default-fixture"))
        .with_test_repository(repository);
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();

    assert!(matches!(
        builder.build(),
        Err(RuntimeRegistrationError::InvalidTestDefinition { id, message })
            if id == "behavioral-test" && message.contains("unregistered-fixture")
    ));
}

#[tokio::test]
async fn behavioral_test_selects_its_fixture_and_only_transports_the_subject() {
    let transport = FakeTransport::accepted("behavioral", false);
    let invocations = Arc::clone(&transport.invocations);
    let reset = Arc::new(RecordingReset::default());
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        behavioral_test_definition(&json!("accepted")),
    ));
    let tracer = behavioral_tracer_with_reset(Arc::new(transport), repository, reset.clone());

    let report = tracer.run_test("behavioral-test").await.unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    assert_eq!(report.revision.as_deref(), Some("test-revision"));
    assert_eq!(reset.fixture_ids.lock().await.as_slice(), ["test-fixture"]);
    assert_eq!(invocations.lock().await.len(), 1);
}

#[tokio::test]
#[allow(clippy::unwrap_used, reason = "test fixture construction must succeed")]
async fn behavioral_comparison_precedes_default_trace_payload_redaction() {
    let store = Arc::new(InMemoryEventStore::new());
    let history: Arc<dyn EventHistory> = store.clone();
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        behavioral_test_definition(&json!("accepted")),
    ));
    let mut builder = builder(history)
        .with_test_event_store(store)
        .with_test_transport(Arc::new(FakeTransport::accepted(
            "behavioral-redacted",
            false,
        )))
        .with_test_scenario_reset(Arc::new(NoopReset))
        .with_default_test_fixture(empty_fixture("default-fixture"))
        .with_test_fixture(empty_fixture("test-fixture"))
        .with_test_repository(repository);
    builder
        .register_json::<TestAggregate, TestCommand>()
        .unwrap();
    let tracer = builder.build().unwrap();

    let report = tracer.run_test("behavioral-test").await.unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    let command = report
        .observed
        .messages()
        .iter()
        .find(|message| message.is_command())
        .unwrap();
    assert_eq!(command.payload(), None);
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
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        behavioral_test_definition(&json!({ "rejected": { "code": "TEST_REJECTION" } })),
    ));
    let tracer = behavioral_tracer(Arc::new(transport), repository);

    let report = tracer.run_test("behavioral-test").await.unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    assert_eq!(
        serde_json::to_value(report.command_outcome.unwrap()).unwrap()["outcome"]["status"],
        "rejected"
    );
}

#[tokio::test]
#[allow(
    clippy::unwrap_used,
    reason = "the test fixture and bounded waits must succeed"
)]
async fn behavioral_timeout_cancels_the_command_before_reset() {
    let mut definition = behavioral_test_definition(&json!("accepted"));
    definition["expected"]["within"] = json!("20ms");
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(definition));
    let tracer = behavioral_tracer(Arc::new(HangingTransport), repository);

    let report = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tracer.run_test("behavioral-test"),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(report.status, TestReportStatus::Failed);
    assert!(
        report
            .comparison
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "timeout-before-expectations")
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tracer.reset_test_scenario(),
    )
    .await
    .unwrap()
    .unwrap();
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
    let mut definition = behavioral_test_definition(&json!("accepted"));
    definition["expected"]["graphs"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "kind": "integration-event",
            "key": "test-published",
            "parentKey": "subject",
            "name": "test-published",
            "schemaVersion": 1
        }));
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(definition));
    let tracer = behavioral_tracer(Arc::new(transport), repository);
    let run_tracer = tracer.clone();
    let run = tokio::spawn(async move { run_tracer.run_test("behavioral-test").await });
    invoked.notified().await;
    let correlation_id = correlation_id.lock().await.clone().unwrap();

    tracer
        .correlation_observer(OperationMode::Test)
        .observe_integration_event(
            &correlation_id,
            IntegrationEventObservation::new(
                "test-published-message",
                "test-published",
                1,
                "test.integration.test-published",
            )
            .with_causation_id("observed-command"),
        )
        .await
        .unwrap();
    let report = run.await.unwrap().unwrap();

    assert_eq!(report.status, TestReportStatus::Passed);
    assert!(report.comparison.matches.iter().any(|matched| {
        matched.expected_key == "test-published"
            && matched.observed_message_id == "test-published-message"
    }));
    assert_eq!(
        report.operation_href,
        format!("/operations/{}", report.operation_id)
    );
    assert_eq!(
        report.correlation_events_href,
        format!("/correlations/{}/events", report.correlation_id)
    );
}

#[tokio::test]
async fn behavioral_deadline_preserves_the_latest_mismatch_diagnostics() {
    let correlation_id = Arc::new(Mutex::new(None));
    let invoked = Arc::new(Notify::new());
    let transport = ObservableTransport {
        correlation_id: Arc::clone(&correlation_id),
        invoked: Arc::clone(&invoked),
    };
    let mut definition = behavioral_test_definition(&json!("accepted"));
    definition["expected"]["within"] = json!("100ms");
    definition["expected"]["graphs"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "kind": "integration-event",
            "key": "test-published",
            "parentKey": "subject",
            "name": "test-published",
            "schemaVersion": 1,
            "payload": { "result": "expected" }
        }));
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(definition));
    let tracer = behavioral_tracer(Arc::new(transport), repository);
    let run_tracer = tracer.clone();
    let run = tokio::spawn(async move { run_tracer.run_test("behavioral-test").await });
    invoked.notified().await;
    let correlation_id = correlation_id.lock().await.clone().unwrap();

    tracer
        .correlation_observer(OperationMode::Test)
        .observe_integration_event(
            &correlation_id,
            IntegrationEventObservation::new(
                "test-published-message",
                "test-published",
                1,
                "test.integration.test-published",
            )
            .with_causation_id("observed-command")
            .with_payload(json!({ "result": "observed" })),
        )
        .await
        .unwrap();
    let report = tokio::time::timeout(std::time::Duration::from_secs(1), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert_eq!(report.status, TestReportStatus::Failed);
    assert!(report.comparison.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "payload-mismatch"
            && diagnostic.path == "expected:test-published/payload"
    }));
    assert!(
        report
            .comparison
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "timeout-before-expectations")
    );
}

#[tokio::test]
async fn behavioral_observation_failure_terminates_without_waiting_for_within() {
    let correlation_id = Arc::new(Mutex::new(None));
    let invoked = Arc::new(Notify::new());
    let transport = ObservableTransport {
        correlation_id: Arc::clone(&correlation_id),
        invoked: Arc::clone(&invoked),
    };
    let mut definition = behavioral_test_definition(&json!("accepted"));
    definition["expected"]["within"] = json!("10s");
    definition["expected"]["graphs"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "kind": "integration-event",
            "key": "test-published",
            "parentKey": "subject",
            "name": "test-published",
            "schemaVersion": 1
        }));
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(definition));
    let tracer = behavioral_tracer(Arc::new(transport), repository);
    let run_tracer = tracer.clone();
    let run = tokio::spawn(async move { run_tracer.run_test("behavioral-test").await });
    invoked.notified().await;
    let correlation_id = correlation_id.lock().await.clone().unwrap();

    tracer
        .correlation_observer(OperationMode::Test)
        .record_observation_failure(
            &correlation_id,
            "malformed-event",
            "stored event checksum is invalid",
        )
        .await
        .unwrap();
    let report = tokio::time::timeout(std::time::Duration::from_secs(1), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();

    assert_eq!(report.status, TestReportStatus::Failed);
    assert!(
        report
            .comparison
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "observation-failure")
    );
}

#[cfg(feature = "http")]
#[tokio::test]
async fn behavioral_invalid_http_documents_return_structured_client_errors() {
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        behavioral_test_definition(&json!("accepted")),
    ));
    let tracer = behavioral_tracer(
        Arc::new(FakeTransport::accepted("behavioral-http", false)),
        repository,
    );
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());

    let malformed = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/tests/validate")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(malformed).await["code"], "invalid-json");

    let unsupported = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/tests/validate")
                .header("content-type", "text/plain")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsupported.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        json_body(unsupported).await["code"],
        "unsupported-media-type"
    );

    let mut semantic = behavioral_test_definition(&json!("accepted"));
    semantic["expected"]["graphs"][0]["nodes"][0]["parentKey"] = json!("missing");
    let invalid = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/test-runs")
                .header("content-type", "application/json")
                .body(Body::from(semantic.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let invalid = json_body(invalid).await;
    assert_eq!(invalid["code"], "invalid-test-definition");
    assert_eq!(invalid["issues"][0]["code"], "unresolved-parent-key");
    assert!(
        invalid["issues"][0]["path"]
            .as_str()
            .unwrap()
            .starts_with("/expected/")
    );

    let mut unknown = behavioral_test_definition(&json!("accepted"));
    unknown["expected"]["graphs"][0]["nodes"][0]["name"] = json!("unknown-command");
    let runtime_invalid = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/tests/validate")
                .header("content-type", "application/json")
                .body(Body::from(unknown.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let runtime_invalid = json_body(runtime_invalid).await;
    assert_eq!(runtime_invalid["code"], "invalid-test-definition");
    assert_eq!(runtime_invalid["issues"][0]["code"], "unknown-command");

    let mut invalid_payload = behavioral_test_definition(&json!("accepted"));
    invalid_payload["expected"]["graphs"][0]["nodes"][0]["payload"]["reject"] =
        json!("not-a-boolean");
    let runtime_invalid = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/test-runs")
                .header("content-type", "application/json")
                .body(Body::from(invalid_payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        json_body(runtime_invalid).await["issues"][0]["code"],
        "invalid-command-payload"
    );
}

#[cfg(feature = "http")]
#[tokio::test]
async fn behavioral_validation_reports_an_unknown_fixture() {
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        behavioral_test_definition(&json!("accepted")),
    ));
    let tracer = behavioral_tracer(
        Arc::new(FakeTransport::accepted("behavioral-http", false)),
        repository,
    );
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());
    let mut definition = behavioral_test_definition(&json!("accepted"));
    definition["setup"]["fixture"] = json!("unknown-fixture");

    let response = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/tests/validate")
                .header("content-type", "application/json")
                .body(Body::from(definition.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "invalid-test-definition");
    assert_eq!(
        body["issues"][0],
        json!({
            "code": "unknown-fixture",
            "path": "/setup/fixture",
            "message": "test definition `behavioral-test` references unknown fixture `unknown-fixture`"
        })
    );
}

#[cfg(feature = "http")]
#[tokio::test]
async fn behavioral_validation_reports_a_command_payload_over_one_mib() {
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(
        behavioral_test_definition(&json!("accepted")),
    ));
    let tracer = behavioral_tracer(
        Arc::new(FakeTransport::accepted("behavioral-http", false)),
        repository,
    );
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());
    let mut definition = behavioral_test_definition(&json!("accepted"));
    let payload = json!({
        "reject": false,
        "padding": "x".repeat(MAX_COMMAND_PAYLOAD_LEN),
    });
    let actual = serde_json::to_vec(&payload).unwrap().len();
    definition["expected"]["graphs"][0]["nodes"][0]["payload"] = payload;
    let document = definition.to_string();
    assert!(document.len() < MAX_COMMAND_PAYLOAD_LEN + 64 * 1024);

    let response = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/tests/validate")
                .header("content-type", "application/json")
                .body(Body::from(document))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(response).await;
    assert_eq!(body["code"], "invalid-test-definition");
    assert_eq!(body["issues"][0]["code"], "command-payload-too-large");
    assert_eq!(
        body["issues"][0]["path"],
        "/expected/graphs/0/nodes/0/payload"
    );
    assert_eq!(
        body["issues"][0]["message"],
        format!(
            "test definition `behavioral-test` payload for command `test-command` is {actual} bytes and exceeds the configured {MAX_COMMAND_PAYLOAD_LEN}-byte limit"
        )
    );
}

#[cfg(feature = "http")]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one hypermedia journey verifies catalog, fixture, schema, validation, and run links"
)]
async fn behavioral_schema_and_validation_are_hypermedia_driven() {
    let definition = behavioral_test_definition(&json!("accepted"));
    let repository: Arc<dyn TestRepository> =
        Arc::new(StaticTestRepository::one(definition.clone()));
    let tracer = behavioral_tracer(
        Arc::new(FakeTransport::accepted("behavioral-http", false)),
        repository,
    );
    let app = http::router(tracer, HttpConfig::new(API_TOKEN).unwrap());

    let catalog = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(catalog.status(), StatusCode::OK);
    let catalog = json_body(catalog).await;
    assert_eq!(catalog["catalogVersion"], 1);
    assert_eq!(
        catalog["testScenario"]["fixturesHref"],
        "/test-scenario/fixtures"
    );
    assert_eq!(
        catalog["testScenario"]["fixtures"],
        json!(["default-fixture", "test-fixture"])
    );
    assert_eq!(
        catalog["behavioralTest"],
        json!({
            "schemaHref": "/schemas/behavioral-test-v1",
            "validateHref": "/tests/validate",
            "runHref": "/test-runs",
            "definitionsHref": "/tests"
        })
    );

    let fixtures = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(catalog["testScenario"]["fixturesHref"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fixtures.status(), StatusCode::OK);
    let fixtures = json_body(fixtures).await;
    assert_eq!(fixtures["items"][0]["id"], "default-fixture");
    assert_eq!(fixtures["items"][0]["isDefault"], true);
    assert_eq!(fixtures["items"][1]["id"], "test-fixture");
    assert_eq!(fixtures["items"][1]["isDefault"], false);

    let fixture = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(fixtures["items"][0]["fixtureHref"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fixture.status(), StatusCode::OK);
    let fixture = json_body(fixture).await;
    assert_eq!(fixture["id"], "default-fixture");
    assert_eq!(fixture["messages"], json!([]));
    let aggregate = &catalog["contexts"][0]["aggregates"][0];
    assert_eq!(
        aggregate["testInstancesHref"],
        "/contexts/test-context/aggregates/test-aggregate/instances"
    );
    assert!(aggregate.get("instancesHref").is_none());
    let version = &aggregate["commands"][0]["versions"][0];
    assert_eq!(
        version["testInputsHrefTemplate"],
        "/contexts/test-context/aggregates/test-aggregate/{aggregateId}/commands/test-command/schemas/1/inputs"
    );
    assert!(version.get("inputsHrefTemplate").is_none());

    let schema = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(catalog["behavioralTest"]["schemaHref"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(schema.status(), StatusCode::OK);
    assert_eq!(schema.headers()["cache-control"], "private, no-store");
    assert_eq!(
        json_body(schema).await,
        serde_json::to_value(rostfrei_tracer::behavioral_test_schema()).unwrap()
    );

    let validation = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri(catalog["behavioralTest"]["validateHref"].as_str().unwrap())
                .header("content-type", "application/json")
                .body(Body::from(definition.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(validation.status(), StatusCode::OK);
    let validation = json_body(validation).await;
    assert_eq!(validation["valid"], true);
    assert_eq!(validation["definition"], definition);
    assert_eq!(validation["schemaHref"], "/schemas/behavioral-test-v1");
    assert_eq!(validation["runHref"], "/test-runs");
}

#[cfg(feature = "http")]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "the test verifies the complete shared report contract for three run outcomes"
)]
async fn inline_and_persisted_behavioral_runs_share_the_report_contract() {
    let passing = behavioral_test_definition(&json!("accepted"));
    let repository: Arc<dyn TestRepository> = Arc::new(StaticTestRepository::one(passing.clone()));
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
    let list = json_body(list).await;
    assert_eq!(list["items"][0]["id"], "behavioral-test");
    assert_eq!(list["items"][0]["runHref"], "/tests/behavioral-test/runs");
    assert_eq!(list["items"][0]["definitionHref"], "/tests/behavioral-test");

    let inline = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/test-runs")
                .header("content-type", "application/json")
                .body(Body::from(passing.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(inline.status(), StatusCode::OK);
    let inline = json_body(inline).await;
    assert_eq!(inline["status"], "passed");
    assert!(inline.get("revision").is_none());
    assert_eq!(inline["comparison"]["status"], "passed");
    assert!(
        !inline["observed"]["messages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        inline["operationHref"],
        format!("/operations/{}", inline["operationId"].as_str().unwrap())
    );
    assert_eq!(
        inline["operationEventsHref"],
        format!(
            "/operations/{}/events",
            inline["operationId"].as_str().unwrap()
        )
    );
    assert_eq!(
        inline["correlationEventsHref"],
        format!(
            "/correlations/{}/events",
            inline["correlationId"].as_str().unwrap()
        )
    );

    let mut failing =
        behavioral_test_definition(&json!({ "rejected": { "code": "TEST_REJECTION" } }));
    failing["expected"]["graphs"][0]["nodes"][0]["payload"]["reject"] = json!(false);
    let failed = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/test-runs")
                .header("content-type", "application/json")
                .body(Body::from(failing.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::OK);
    let failed = json_body(failed).await;
    assert_eq!(failed["status"], "failed");
    assert!(
        failed["comparison"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "command-outcome-mismatch")
    );

    let persisted = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri(list["items"][0]["runHref"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(persisted.status(), StatusCode::OK);
    assert_eq!(persisted.headers()["cache-control"], "private, no-store");
    let persisted = json_body(persisted).await;
    assert_eq!(persisted["status"], "passed");
    assert_eq!(persisted["revision"], "test-revision");
    for field in [
        "expected",
        "observed",
        "comparison",
        "operationHref",
        "operationEventsHref",
        "correlationEventsHref",
    ] {
        assert!(persisted.get(field).is_some(), "missing {field}");
    }
}
rostfrei::install_macro_support!();
