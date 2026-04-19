use std::{error::Error, sync::Arc};

use async_trait::async_trait;
#[cfg(feature = "http")]
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
#[cfg(feature = "http")]
use http_body_util::BodyExt as _;
use rostfrei_control_plane::{
    CommandWireCodec, CommandWireCodecError, ControlPlane, ControlPlaneBuilder, DispatchAdapter,
    DispatchError, DispatchErrorKind, DispatchInvocation, DispatchObserver, DispatchPublication,
    DispatchReceipt, DispatchRejection, DispatchRequest, ExposeTracePayloadsForLocalDevelopment,
    RuntimeRegistrationError, SimulationRequest, SubmissionError, SubscriptionError,
};
#[cfg(feature = "http")]
use rostfrei_control_plane::{
    MAX_COMMAND_PAYLOAD_LEN,
    http::{self, DispatchHttpConfig, HttpConfig},
};
use rostfrei_core::{
    Aggregate, AggregateInstance, CommandHandler, Event, EventCodec, EventCodecError,
    EventCodecErrorKind, EventHistory, EventId, EventStoreError, EventStoreErrorKind,
    InMemoryEventStore, NewEvent, RecordedEvent, StreamId,
};
use rostfrei_registry::{CommandDefinition, DomainModule, DomainRegistry, ModuleDescriptor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};
#[cfg(feature = "http")]
use tower::ServiceExt as _;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const AGGREGATE_TYPE: &str = "test-context/test-aggregate";
const OTHER_AGGREGATE_TYPE: &str = "test-context/other-aggregate";
const COMMAND_NAME: &str = "test-command";
#[cfg(feature = "http")]
const API_TOKEN: &str = "integration-test-capability";
#[cfg(feature = "http")]
const DISPATCH_TOKEN: &str = "integration-test-dispatch-capability";
#[cfg(feature = "http")]
const SIMULATION_PATH: &str = "/v1/contexts/test-context/aggregates/test-aggregate/aggregate-1/commands/test-command/simulate";
#[cfg(feature = "http")]
const DISPATCH_PATH: &str = "/v1/contexts/test-context/aggregates/test-aggregate/aggregate-1/commands/test-command/dispatch";

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

struct TestCommand {
    reject: bool,
}

impl CommandDefinition for TestCommand {
    type Aggregate = TestAggregate;

    const COMMAND_NAME: &'static str = COMMAND_NAME;
    const SCHEMA_VERSION: u32 = 1;
}

struct TestRejection;

impl CommandHandler<TestCommand> for TestAggregate {
    type Rejection = TestRejection;

    fn handle(
        command: &TestCommand,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        if command.reject {
            return Err(TestRejection);
        }
        aggregate.raise(TestEvent {
            sensitive: "accepted outcome details".to_owned(),
        });
        Ok(())
    }
}

struct OtherTestAggregate;

impl Aggregate for OtherTestAggregate {
    type State = ();
    type Event = TestEvent;

    const AGGREGATE_TYPE: &'static str = OTHER_AGGREGATE_TYPE;

    fn initial(_stream_id: &StreamId) -> Self::State {}

    fn apply(_state: &mut Self::State, _event: &Self::Event) {}
}

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

#[derive(Clone, Copy)]
struct TestWireCodec;

impl CommandWireCodec<TestCommand> for TestWireCodec {
    fn decode(&self, payload: &Value) -> Result<TestCommand, CommandWireCodecError> {
        let reject = payload
            .get("reject")
            .and_then(Value::as_bool)
            .ok_or_else(|| CommandWireCodecError::new("reject must be a boolean"))?;
        Ok(TestCommand { reject })
    }

    fn encode_rejection(&self, _rejection: &TestRejection) -> Result<Value, CommandWireCodecError> {
        Ok(json!({
            "code": "TEST_REJECTION",
            "detail": "rejected outcome details",
        }))
    }
}

impl CommandWireCodec<OtherTestCommand> for TestWireCodec {
    fn decode(&self, payload: &Value) -> Result<OtherTestCommand, CommandWireCodecError> {
        let reject = payload
            .get("reject")
            .and_then(Value::as_bool)
            .ok_or_else(|| CommandWireCodecError::new("reject must be a boolean"))?;
        Ok(OtherTestCommand { reject })
    }

    fn encode_rejection(&self, _rejection: &TestRejection) -> Result<Value, CommandWireCodecError> {
        Ok(json!({
            "code": "TEST_REJECTION",
            "detail": "rejected outcome details",
        }))
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

fn builder(history: Arc<dyn EventHistory>) -> TestResult<ControlPlaneBuilder> {
    let mut registry = DomainRegistry::new();
    registry.register_module::<TestDomainModule>()?;
    Ok(ControlPlaneBuilder::with_registry(history, registry))
}

fn control_plane(maximum_operations: usize) -> TestResult<ControlPlane> {
    let mut builder =
        builder(Arc::new(InMemoryEventStore::new()))?.with_maximum_operations(maximum_operations);
    builder.register::<TestCommand, _>(TestWireCodec)?;
    Ok(builder.build()?)
}

#[derive(Clone)]
struct RecordingDispatcher {
    invocations: Arc<Mutex<Vec<DispatchInvocation>>>,
    result: Result<DispatchReceipt, DispatchError>,
}

impl RecordingDispatcher {
    fn successful() -> Self {
        Self {
            invocations: Arc::new(Mutex::new(Vec::new())),
            result: Ok(DispatchReceipt::accepted(
                "command-message-1",
                "response-message-1",
                false,
            )),
        }
    }
}

#[async_trait]
impl DispatchAdapter for RecordingDispatcher {
    async fn dispatch(
        &self,
        invocation: DispatchInvocation,
        observer: Arc<dyn DispatchObserver>,
    ) -> Result<DispatchReceipt, DispatchError> {
        self.invocations.lock().await.push(invocation);
        if let Ok(receipt) = &self.result {
            observer
                .command_published(DispatchPublication::new(
                    receipt.command_message_id(),
                    receipt.duplicate(),
                ))
                .await;
        }
        self.result.clone()
    }
}

fn dispatch_control_plane(dispatcher: impl DispatchAdapter + 'static) -> TestResult<ControlPlane> {
    let mut builder = builder(Arc::new(InMemoryEventStore::new()))?;
    builder.register::<TestCommand, _>(TestWireCodec)?;
    builder.register_dispatch::<TestCommand>(Arc::new(dispatcher))?;
    Ok(builder.build()?)
}

struct LimitedDispatcher;

#[async_trait]
impl DispatchAdapter for LimitedDispatcher {
    fn maximum_payload_len(&self) -> usize {
        4
    }

    async fn dispatch(
        &self,
        _invocation: DispatchInvocation,
        observer: Arc<dyn DispatchObserver>,
    ) -> Result<DispatchReceipt, DispatchError> {
        observer
            .command_published(DispatchPublication::new("limited-command", false))
            .await;
        Ok(DispatchReceipt::accepted(
            "limited-command",
            "limited-response",
            false,
        ))
    }
}

async fn submit(control_plane: &ControlPlane, operation_id: &str, payload: Value) -> TestResult {
    control_plane
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
        .await?;
    Ok(())
}

async fn terminal_operation(control_plane: &ControlPlane, operation_id: &str) -> TestResult<Value> {
    let (operation, _) = terminal_operation_with_trace(control_plane, operation_id).await?;
    Ok(operation)
}

async fn terminal_operation_with_trace(
    control_plane: &ControlPlane,
    operation_id: &str,
) -> TestResult<(Value, String)> {
    let mut subscription = control_plane.subscribe(operation_id, 0).await?;
    let mut trace = String::new();
    while let Some(event) = subscription.next().await {
        trace.push_str(&serde_json::to_string(&event)?);
    }
    Ok((
        serde_json::to_value(control_plane.operation(operation_id).await?)?,
        trace,
    ))
}

#[tokio::test]
async fn accepted_dispatch_traces_publication_before_terminal_response_and_is_idempotent() {
    let dispatcher = RecordingDispatcher::successful();
    let invocations = Arc::clone(&dispatcher.invocations);
    let control_plane = dispatch_control_plane(dispatcher).unwrap();
    let request = || DispatchRequest {
        schema_version: 1,
        payload: json!({ "reject": false }),
    };

    control_plane
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            request(),
            "dispatch-operation",
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&control_plane, "dispatch-operation")
        .await
        .unwrap();

    assert_eq!(operation["mode"], "dispatch");
    assert_eq!(operation["result"]["decision"], "accepted");
    assert!(operation["result"].get("appended").is_none());
    assert_eq!(operation["result"]["published"], true);
    assert_eq!(operation["result"]["commandMessageId"], "command-message-1");
    assert_eq!(
        operation["result"]["responseMessageId"],
        "response-message-1"
    );
    assert_eq!(operation["result"]["duplicate"], false);
    assert!(operation["result"].get("baseStreamVersion").is_none());
    assert!(trace.contains("command-published"));
    assert!(trace.contains("command-message-1"));
    assert!(trace.contains("command-responded"));
    assert!(trace.contains("response-message-1"));
    assert!(!trace.contains("history-replayed"));
    assert!(trace.contains("command-accepted"));
    assert!(matches!(
        (
            trace.find("command-published"),
            trace.find("command-responded"),
            trace.find("command-accepted"),
            trace.find("completed"),
        ),
        (Some(published), Some(responded), Some(accepted), Some(completed))
            if published < responded && responded < accepted && accepted < completed
    ));

    control_plane
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            request(),
            "dispatch-operation",
        )
        .await
        .unwrap();
    let invocation_count = invocations.lock().await.len();
    assert_eq!(invocation_count, 1);
    assert_eq!(
        control_plane
            .submit_dispatch(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                DispatchRequest {
                    schema_version: 1,
                    payload: json!({ "reject": true }),
                },
                "dispatch-operation",
            )
            .await,
        Err(SubmissionError::IdentityConflict)
    );

    let invocations = invocations.lock().await;
    assert_eq!(
        invocations
            .first()
            .map(|invocation| invocation.operation_id().as_str()),
        Some("dispatch-operation")
    );
    assert_eq!(
        invocations.first().map(DispatchInvocation::aggregate_type),
        Some(AGGREGATE_TYPE)
    );
    assert_eq!(
        invocations
            .first()
            .map(|invocation| invocation.aggregate_id().as_str()),
        Some("aggregate-1")
    );
    assert_eq!(
        invocations.first().map(DispatchInvocation::command),
        Some(COMMAND_NAME)
    );
}

#[tokio::test]
async fn rejected_dispatch_is_terminal_and_obeys_the_trace_payload_policy() {
    let rejection = DispatchRejection::new(
        "conflict",
        "TEST_REJECTION",
        "private rejection message",
        Some(json!({ "private": "details" })),
    );
    let dispatcher = RecordingDispatcher {
        invocations: Arc::new(Mutex::new(Vec::new())),
        result: Ok(DispatchReceipt::rejected(
            "rejected-command",
            "rejected-response",
            true,
            rejection.clone(),
        )),
    };
    let control_plane = dispatch_control_plane(dispatcher).unwrap();
    control_plane
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            DispatchRequest {
                schema_version: 1,
                payload: json!({ "reject": true }),
            },
            "rejected-dispatch",
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&control_plane, "rejected-dispatch")
        .await
        .unwrap();
    assert_eq!(operation["result"]["decision"], "rejected");
    assert!(operation["result"].get("appended").is_none());
    assert_eq!(operation["result"]["published"], true);
    assert_eq!(operation["result"]["commandMessageId"], "rejected-command");
    assert_eq!(
        operation["result"]["responseMessageId"],
        "rejected-response"
    );
    assert_eq!(operation["result"]["duplicate"], true);
    assert_eq!(
        operation["result"]["rejection"],
        json!({ "redacted": true })
    );
    assert!(trace.contains("command-responded"));
    assert!(trace.contains("command-rejected"));
    assert!(!trace.contains("private rejection message"));
    assert!(!trace.contains("private\":\"details"));

    let mut builder = builder(Arc::new(InMemoryEventStore::new()))
        .unwrap()
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register::<TestCommand, _>(TestWireCodec).unwrap();
    builder
        .register_dispatch::<TestCommand>(Arc::new(RecordingDispatcher {
            invocations: Arc::new(Mutex::new(Vec::new())),
            result: Ok(DispatchReceipt::rejected(
                "exposed-command",
                "exposed-response",
                false,
                rejection,
            )),
        }))
        .unwrap();
    let exposed = builder.build().unwrap();
    exposed
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            DispatchRequest {
                schema_version: 1,
                payload: json!({ "reject": true }),
            },
            "exposed-rejection",
        )
        .await
        .unwrap();
    let exposed = terminal_operation(&exposed, "exposed-rejection")
        .await
        .unwrap();
    assert_eq!(exposed["result"]["rejection"]["classification"], "conflict");
    assert_eq!(exposed["result"]["rejection"]["code"], "TEST_REJECTION");
    assert_eq!(
        exposed["result"]["rejection"]["details"],
        json!({ "private": "details" })
    );
}

#[tokio::test]
async fn dispatch_failures_are_stable_and_redacted() {
    let dispatcher = RecordingDispatcher {
        invocations: Arc::new(Mutex::new(Vec::new())),
        result: Err(DispatchError::new(
            DispatchErrorKind::Unavailable,
            "private broker failure",
        )),
    };
    let control_plane = dispatch_control_plane(dispatcher).unwrap();

    control_plane
        .submit_dispatch(
            AGGREGATE_TYPE,
            "aggregate-1",
            COMMAND_NAME,
            DispatchRequest {
                schema_version: 1,
                payload: json!({ "reject": false }),
            },
            "failed-dispatch",
        )
        .await
        .unwrap();
    let (operation, trace) = terminal_operation_with_trace(&control_plane, "failed-dispatch")
        .await
        .unwrap();
    assert_eq!(operation["failure"]["code"], "dispatch-unavailable");
    assert_eq!(
        operation["failure"]["message"],
        "operation failure details are redacted"
    );
    assert!(!trace.contains("private broker failure"));
}

#[tokio::test]
async fn dispatch_admission_honors_the_adapter_payload_limit() {
    let control_plane = dispatch_control_plane(LimitedDispatcher).unwrap();

    assert_eq!(
        control_plane
            .submit_dispatch(
                AGGREGATE_TYPE,
                "aggregate-1",
                COMMAND_NAME,
                DispatchRequest {
                    schema_version: 1,
                    payload: json!(false),
                },
                "limited-dispatch",
            )
            .await,
        Err(SubmissionError::PayloadTooLarge { maximum: 4 })
    );
}

#[test]
fn dispatch_registration_requires_and_preserves_a_simulation_binding() {
    let dispatcher: Arc<dyn DispatchAdapter> = Arc::new(RecordingDispatcher::successful());
    let history: Arc<dyn EventHistory> = Arc::new(InMemoryEventStore::new());
    let mut missing_simulation = builder(Arc::clone(&history)).unwrap();
    assert!(matches!(
        missing_simulation.register_dispatch::<TestCommand>(Arc::clone(&dispatcher)),
        Err(RuntimeRegistrationError::DispatchWithoutSimulationBinding {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));

    let mut duplicate_dispatch = builder(history).unwrap();
    duplicate_dispatch
        .register::<TestCommand, _>(TestWireCodec)
        .unwrap();
    duplicate_dispatch
        .register_dispatch::<TestCommand>(Arc::clone(&dispatcher))
        .unwrap();
    assert!(matches!(
        duplicate_dispatch.register_dispatch::<TestCommand>(dispatcher),
        Err(RuntimeRegistrationError::DuplicateDispatchBinding {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));
}

#[cfg(feature = "http")]
fn authorize(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header("authorization", format!("Bearer {API_TOKEN}"))
}

#[cfg(feature = "http")]
async fn json_body(response: axum::response::Response) -> TestResult<Value> {
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http_requires_a_bearer_capability_and_reports_invalid_input() {
    let app = http::router(
        control_plane(1024).unwrap(),
        HttpConfig::new(API_TOKEN).unwrap(),
    );

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/operations/not-present")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(unauthorized.headers()["www-authenticate"], "Bearer");

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
    assert_eq!(json_body(malformed).await.unwrap()["code"], "invalid-json");

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
    assert_eq!(
        json_body(oversized).await.unwrap()["code"],
        "payload-too-large"
    );
}

#[cfg(feature = "http")]
#[tokio::test]
async fn dispatch_http_uses_a_separate_capability_and_requires_idempotency() {
    let control_plane = dispatch_control_plane(RecordingDispatcher::successful()).unwrap();
    let app = http::router(control_plane.clone(), HttpConfig::new(API_TOKEN).unwrap()).merge(
        http::dispatch_router(
            control_plane,
            DispatchHttpConfig::new(DISPATCH_TOKEN).unwrap(),
        ),
    );
    let body = || {
        Body::from(
            json!({
                "schemaVersion": 1,
                "payload": { "reject": false },
            })
            .to_string(),
        )
    };

    let wrong_capability = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri(DISPATCH_PATH)
                .header("content-type", "application/json")
                .header("idempotency-key", "http-dispatch")
                .body(body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_capability.status(), StatusCode::UNAUTHORIZED);

    let missing_key = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(DISPATCH_PATH)
                .header("authorization", format!("Bearer {DISPATCH_TOKEN}"))
                .header("content-type", "application/json")
                .body(body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_key.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_key).await.unwrap()["code"],
        "idempotency-key-required"
    );

    let accepted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(DISPATCH_PATH)
                .header("authorization", format!("Bearer {DISPATCH_TOKEN}"))
                .header("content-type", "application/json")
                .header("idempotency-key", "http-dispatch")
                .body(body())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    assert_eq!(json_body(accepted).await.unwrap()["mode"], "dispatch");

    let completion = app
        .oneshot(
            authorize(Request::builder())
                .uri("/v1/operations/http-dispatch/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(completion.status(), StatusCode::OK);
    let completion = String::from_utf8(
        completion
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(completion.contains("event: command.published"));
    assert!(completion.contains("event: command.responded"));
    assert!(completion.contains("event: command.accepted"));
    assert!(completion.contains("event: operation.completed"));
}

#[tokio::test]
async fn default_policy_redacts_results_and_terminal_operations_are_evicted() {
    let control_plane = control_plane(1).unwrap();

    submit(
        &control_plane,
        "redacted-accepted",
        json!({ "reject": false }),
    )
    .await
    .unwrap();
    let (accepted, accepted_trace) =
        terminal_operation_with_trace(&control_plane, "redacted-accepted")
            .await
            .unwrap();
    assert!(
        accepted["result"]["predictedEvents"][0]
            .get("payload")
            .is_none()
    );
    assert!(
        accepted["result"]["predictedEvents"][0]
            .get("payloadBase64")
            .is_none()
    );
    assert!(!accepted_trace.contains("accepted outcome details"));

    submit(
        &control_plane,
        "redacted-rejected",
        json!({ "reject": true }),
    )
    .await
    .unwrap();
    assert_eq!(
        control_plane.operation("redacted-accepted").await,
        Err(SubmissionError::NotFound)
    );
    let (rejected, rejected_trace) =
        terminal_operation_with_trace(&control_plane, "redacted-rejected")
            .await
            .unwrap();
    assert_eq!(rejected["result"]["rejection"], json!({ "redacted": true }));
    assert!(!rejected_trace.contains("rejected outcome details"));

    submit(
        &control_plane,
        "redacted-failure",
        json!({ "reject": "not-a-boolean" }),
    )
    .await
    .unwrap();
    let (failure, failure_trace) =
        terminal_operation_with_trace(&control_plane, "redacted-failure")
            .await
            .unwrap();
    assert_eq!(failure["failure"]["code"], "invalid-command-payload");
    assert_eq!(
        failure["failure"]["message"],
        "operation failure details are redacted"
    );
    assert!(!failure_trace.contains("reject must be a boolean"));
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
    let mut builder =
        ControlPlaneBuilder::with_registry(Arc::new(InMemoryEventStore::new()), registry);
    builder.register::<TestCommand, _>(TestWireCodec).unwrap();
    builder
        .register::<OtherTestCommand, _>(TestWireCodec)
        .unwrap();
    let control_plane = builder.build().unwrap();

    for (aggregate_type, operation_id) in [
        (AGGREGATE_TYPE, "first-aggregate-command"),
        (OTHER_AGGREGATE_TYPE, "second-aggregate-command"),
    ] {
        control_plane
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
        terminal_operation(&control_plane, operation_id)
            .await
            .unwrap();
    }
}

#[test]
fn runtime_binding_rejects_a_registry_descriptor_for_a_different_command_contract() {
    let mut registry = DomainRegistry::new();
    registry
        .register_module::<MismatchedTestDomainModule>()
        .unwrap();
    let mut builder =
        ControlPlaneBuilder::with_registry(Arc::new(InMemoryEventStore::new()), registry);

    assert!(matches!(
        builder.register::<TestCommand, _>(TestWireCodec),
        Err(RuntimeRegistrationError::DescriptorMismatch {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));
}

#[test]
fn runtime_bindings_require_exact_registry_coverage() {
    let history: Arc<dyn EventHistory> = Arc::new(InMemoryEventStore::new());
    let mut empty_registry_builder = ControlPlaneBuilder::new(Arc::clone(&history));
    empty_registry_builder
        .register::<TestCommand, _>(TestWireCodec)
        .unwrap();
    empty_registry_builder.build().unwrap();

    let missing_binding = builder(Arc::clone(&history)).unwrap();
    assert!(matches!(
        missing_binding.build(),
        Err(RuntimeRegistrationError::MissingBinding {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));

    let mut duplicate_binding = builder(history).unwrap();
    duplicate_binding
        .register::<TestCommand, _>(TestWireCodec)
        .unwrap();
    assert!(matches!(
        duplicate_binding.register::<TestCommand, _>(TestWireCodec),
        Err(RuntimeRegistrationError::DuplicateBinding {
            command: COMMAND_NAME,
            schema_version: 1,
        })
    ));
}

#[test]
fn excessive_concurrency_configuration_does_not_panic() {
    let mut builder = builder(Arc::new(InMemoryEventStore::new()))
        .unwrap()
        .with_maximum_operations(usize::MAX)
        .with_maximum_concurrent_simulations(usize::MAX);
    builder.register::<TestCommand, _>(TestWireCodec).unwrap();

    builder.build().unwrap();
}

#[tokio::test]
async fn generated_operation_ids_are_distinct_and_valid() {
    let control_plane = control_plane(4).unwrap();
    let request = || SimulationRequest {
        schema_version: 1,
        payload: json!({ "reject": false }),
    };

    let first = control_plane
        .submit_simulation(AGGREGATE_TYPE, "aggregate-1", COMMAND_NAME, request(), None)
        .await
        .unwrap();
    let second = control_plane
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

fn blocking_control_plane(
    maximum_operations: usize,
    maximum_concurrent_simulations: usize,
) -> TestResult<(ControlPlane, Arc<Notify>, Arc<Notify>)> {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let history: Arc<dyn EventHistory> = Arc::new(BlockingHistory {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let mut builder = builder(history)?
        .with_maximum_operations(maximum_operations)
        .with_maximum_concurrent_simulations(maximum_concurrent_simulations);
    builder.register::<TestCommand, _>(TestWireCodec)?;
    Ok((builder.build()?, entered, release))
}

fn simulation_request(reject: bool) -> SimulationRequest {
    SimulationRequest {
        schema_version: 1,
        payload: json!({ "reject": reject }),
    }
}

#[tokio::test]
async fn concurrent_admission_is_bounded_before_operation_capacity() {
    let (control_plane, entered, release) = blocking_control_plane(4, 1).unwrap();
    control_plane
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

    let repeated = control_plane
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
        control_plane
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
    terminal_operation(&control_plane, "running-operation")
        .await
        .unwrap();
}

#[tokio::test]
async fn operation_capacity_rejects_work_when_no_terminal_record_can_be_evicted() {
    let (control_plane, entered, release) = blocking_control_plane(1, 1).unwrap();
    control_plane
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
        control_plane
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
    terminal_operation(&control_plane, "capacity-running")
        .await
        .unwrap();
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
        let mut builder = builder(Arc::new(FailedHistory(kind))).unwrap();
        builder.register::<TestCommand, _>(TestWireCodec).unwrap();
        let control_plane = builder.build().unwrap();
        submit(
            &control_plane,
            &format!("failed-history-{expected_code}"),
            json!({ "reject": false }),
        )
        .await
        .unwrap();
        let operation =
            terminal_operation(&control_plane, &format!("failed-history-{expected_code}"))
                .await
                .unwrap();
        assert_eq!(operation["failure"]["code"], expected_code);
    }
}

#[derive(Clone, Copy)]
struct FailingEventCodec;

impl EventCodec<TestAggregate> for FailingEventCodec {
    fn encode(&self, _event: &TestEvent, _event_id: EventId) -> Result<NewEvent, EventCodecError> {
        Err(EventCodecError::new(
            EventCodecErrorKind::EncodingFailed,
            "prediction encoding failed",
        ))
    }

    fn decode(&self, _event: &RecordedEvent) -> Result<TestEvent, EventCodecError> {
        Err(EventCodecError::new(
            EventCodecErrorKind::MalformedPayload,
            "unexpected event in empty test history",
        ))
    }
}

#[tokio::test]
async fn event_codec_failures_have_a_stable_code() {
    let mut builder = builder(Arc::new(InMemoryEventStore::new())).unwrap();
    builder
        .register_with_codec::<TestCommand, _, _>(FailingEventCodec, TestWireCodec)
        .unwrap();
    let control_plane = builder.build().unwrap();
    submit(&control_plane, "codec-failure", json!({ "reject": false }))
        .await
        .unwrap();

    let operation = terminal_operation(&control_plane, "codec-failure")
        .await
        .unwrap();
    assert_eq!(operation["failure"]["code"], "event-codec-failed");
}

#[derive(Clone, Copy)]
struct BinaryEventCodec;

impl EventCodec<TestAggregate> for BinaryEventCodec {
    fn encode(&self, _event: &TestEvent, event_id: EventId) -> Result<NewEvent, EventCodecError> {
        NewEvent::new(event_id, "test-event", 1, vec![0xff, 0x00, 0x01]).map_err(|error| {
            EventCodecError::new(EventCodecErrorKind::InvalidEnvelope, error.to_string())
        })
    }

    fn decode(&self, _event: &RecordedEvent) -> Result<TestEvent, EventCodecError> {
        Err(EventCodecError::new(
            EventCodecErrorKind::MalformedPayload,
            "unexpected event in empty test history",
        ))
    }
}

#[tokio::test]
async fn non_json_predictions_are_exposed_as_base64_only_when_explicitly_enabled() {
    let mut builder = builder(Arc::new(InMemoryEventStore::new()))
        .unwrap()
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder
        .register_with_codec::<TestCommand, _, _>(BinaryEventCodec, TestWireCodec)
        .unwrap();
    let control_plane = builder.build().unwrap();
    submit(
        &control_plane,
        "binary-prediction",
        json!({ "reject": false }),
    )
    .await
    .unwrap();

    let operation = terminal_operation(&control_plane, "binary-prediction")
        .await
        .unwrap();
    let event = &operation["result"]["predictedEvents"][0];
    assert!(event.get("payload").is_none());
    assert_eq!(event["payloadBase64"], "/wAB");
}

#[derive(Clone, Copy)]
struct PanickingWireCodec;

struct DeliberateCommandCodecPanic;

impl CommandWireCodec<TestCommand> for PanickingWireCodec {
    fn decode(&self, _payload: &Value) -> Result<TestCommand, CommandWireCodecError> {
        std::panic::resume_unwind(Box::new(DeliberateCommandCodecPanic))
    }

    fn encode_rejection(&self, _rejection: &TestRejection) -> Result<Value, CommandWireCodecError> {
        Err(CommandWireCodecError::new(
            "rejection encoding is unavailable after command decoding panics",
        ))
    }
}

#[tokio::test]
async fn panics_become_one_terminal_failure_and_release_admission() {
    let mut builder = builder(Arc::new(InMemoryEventStore::new()))
        .unwrap()
        .with_maximum_concurrent_simulations(1);
    builder
        .register::<TestCommand, _>(PanickingWireCodec)
        .unwrap();
    let control_plane = builder.build().unwrap();
    submit(&control_plane, "panicking-operation", json!({}))
        .await
        .unwrap();

    let operation = terminal_operation(&control_plane, "panicking-operation")
        .await
        .unwrap();
    assert_eq!(operation["status"], "failed");
    assert_eq!(operation["failure"]["code"], "simulation-panicked");
    assert_eq!(operation["latestEventId"], 3);

    submit(&control_plane, "panicking-operation-2", json!({}))
        .await
        .unwrap();
    let second = terminal_operation(&control_plane, "panicking-operation-2")
        .await
        .unwrap();
    assert_eq!(second["failure"]["code"], "simulation-panicked");
}

#[tokio::test]
async fn future_and_terminal_operation_cursors_are_explicit() {
    let control_plane = control_plane(4).unwrap();
    submit(
        &control_plane,
        "cursor-operation",
        json!({ "reject": false }),
    )
    .await
    .unwrap();
    let operation = terminal_operation(&control_plane, "cursor-operation")
        .await
        .unwrap();
    let latest = operation["latestEventId"].as_u64().unwrap();

    let terminal = control_plane
        .subscribe("cursor-operation", latest)
        .await
        .unwrap();
    assert!(terminal.is_complete().await);
    assert!(matches!(
        control_plane.subscribe("cursor-operation", latest + 1).await,
        Err(SubmissionError::InvalidCursor(
            SubscriptionError::FutureCursor { latest: actual }
        )) if actual == latest
    ));
}

#[cfg(feature = "http")]
#[tokio::test]
async fn http_reports_future_sse_cursors_with_a_stable_code() {
    let control_plane = control_plane(4).unwrap();
    submit(
        &control_plane,
        "http-cursor-operation",
        json!({ "reject": false }),
    )
    .await
    .unwrap();
    let operation = terminal_operation(&control_plane, "http-cursor-operation")
        .await
        .unwrap();
    let latest = operation["latestEventId"].as_u64().unwrap();
    let app = http::router(control_plane, HttpConfig::new(API_TOKEN).unwrap());

    let response = app
        .oneshot(
            authorize(Request::builder())
                .uri("/v1/operations/http-cursor-operation/events")
                .header("last-event-id", (latest + 1).to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await.unwrap()["code"], "future-cursor");
}
