#![allow(
    clippy::indexing_slicing,
    clippy::needless_pass_by_value,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "HTTP integration fixtures use fixed valid inputs and end-to-end assertions"
)]

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bike_rental::{
    demo::{demo_stream, seed_demo},
    domain_model,
    rental_fleet::{AddBicycle, RentBicycle, RentalFleetAggregate, ReturnBicycle},
    tracer::{self, RentBicycleInputOptions, ReturnBicycleInputOptions},
};
use http_body_util::BodyExt as _;
use rostfrei::{
    Aggregate, AppendOutcome, CommandDefinition, CommandExecutionError, ContentFingerprint,
    DomainErrorType, EventBatch, EventHistory, EventStore, EventStoreError, EventTransaction,
    ExecutionMetadata, Executor, ExpectedVersion, InMemoryEventStore, JsonCommandPayload,
    JsonErrorPayload, OperationId, StreamAggregateId, StreamAggregateType, StreamId,
    TransactionAppendOutcome, TransactionReceipt,
};
use rostfrei_core::{StreamDirectory, StreamSummary};
use rostfrei_messaging_core::CorrelationId;
use rostfrei_tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandReceipt, CommandRejection,
    CommandTransport, CommandTransportError, CommandTransportErrorKind, CommandTransportObserver,
    ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository, TestRepository,
    TestScenarioReset, TestScenarioResetError, Tracer,
    http::{self, HttpConfig},
};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

const API_TOKEN: &str = "integration-test-capability";
const DISPATCH_TOKEN: &str = "integration-dispatch-capability";

#[derive(Clone)]
struct ResettableStore {
    store: Arc<tokio::sync::RwLock<InMemoryEventStore>>,
}

impl ResettableStore {
    fn new() -> Self {
        Self {
            store: Arc::new(tokio::sync::RwLock::new(InMemoryEventStore::new())),
        }
    }

    async fn snapshot(&self) -> InMemoryEventStore {
        self.store.read().await.clone()
    }

    async fn reset_and_seed(&self) -> Result<(), TestScenarioResetError> {
        *self.store.write().await = InMemoryEventStore::new();
        seed_demo(self)
            .await
            .map_err(|error| TestScenarioResetError::Failed(error.to_string()))
    }
}

#[async_trait]
impl EventHistory for ResettableStore {
    async fn load(
        &self,
        stream_id: &StreamId,
    ) -> Result<Vec<rostfrei::RecordedEvent>, EventStoreError> {
        self.snapshot().await.load(stream_id).await
    }
}

#[async_trait]
impl EventStore for ResettableStore {
    async fn append(
        &self,
        stream_id: &StreamId,
        expected_version: ExpectedVersion,
        batch: EventBatch,
    ) -> Result<AppendOutcome, EventStoreError> {
        self.snapshot()
            .await
            .append(stream_id, expected_version, batch)
            .await
    }

    async fn load_transaction_receipt(
        &self,
        primary_stream_id: &StreamId,
        operation_id: &OperationId,
    ) -> Result<Option<TransactionReceipt>, EventStoreError> {
        self.snapshot()
            .await
            .load_transaction_receipt(primary_stream_id, operation_id)
            .await
    }

    async fn append_transaction(
        &self,
        transaction: EventTransaction,
    ) -> Result<TransactionAppendOutcome, EventStoreError> {
        self.snapshot().await.append_transaction(transaction).await
    }
}

#[async_trait]
impl StreamDirectory for ResettableStore {
    async fn list_streams(
        &self,
        aggregate_type: &StreamAggregateType,
    ) -> Result<Vec<StreamSummary>, EventStoreError> {
        self.snapshot().await.list_streams(aggregate_type).await
    }
}

#[async_trait]
impl TestScenarioReset for ResettableStore {
    async fn reset(&self) -> Result<(), TestScenarioResetError> {
        self.reset_and_seed().await
    }
}

#[derive(Clone)]
struct LocalCommandTransport<Store> {
    store: Store,
}

impl<Store> LocalCommandTransport<Store> {
    const fn new(store: Store) -> Self {
        Self { store }
    }
}

#[async_trait]
impl<Store> CommandTransport for LocalCommandTransport<Store>
where
    Store: EventStore + Clone + Send + Sync + 'static,
{
    async fn invoke(
        &self,
        invocation: CommandInvocation,
        observer: Arc<dyn CommandTransportObserver>,
    ) -> Result<CommandReceipt, CommandTransportError> {
        if invocation.aggregate_type() != RentalFleetAggregate::aggregate_type().as_ref() {
            return Err(local_transport_error(
                CommandTransportErrorKind::InvalidRequest,
                "unexpected aggregate type",
            ));
        }
        let stream = StreamId::new(
            StreamAggregateType::new(invocation.aggregate_type()).map_err(|error| {
                local_transport_error(CommandTransportErrorKind::InvalidRequest, error.to_string())
            })?,
            StreamAggregateId::new(invocation.aggregate_id().as_str()).map_err(|error| {
                local_transport_error(CommandTransportErrorKind::InvalidRequest, error.to_string())
            })?,
        );
        let command_message_id = ContentFingerprint::digest(format!(
            "local-command:{}:{}:{}",
            invocation.operation_id().as_str(),
            invocation.correlation_id(),
            invocation.execution_fingerprint().to_hex()
        ))
        .to_hex();
        observer
            .command_published(CommandPublication::new(&command_message_id, false))
            .await;
        let correlation_id = CorrelationId::new(invocation.correlation_id()).map_err(|error| {
            local_transport_error(CommandTransportErrorKind::InvalidRequest, error.to_string())
        })?;
        let metadata = ExecutionMetadata::new(
            stream,
            invocation.operation_id().clone(),
            invocation.execution_fingerprint(),
        )
        .with_correlation_id(correlation_id);
        let outcome = match (invocation.command(), invocation.schema_version()) {
            (RentBicycle::COMMAND_NAME, RentBicycle::SCHEMA_VERSION) => {
                let command = RentBicycle::decode_json(invocation.payload()).map_err(|error| {
                    local_transport_error(CommandTransportErrorKind::InvalidRequest, error)
                })?;
                match Executor::new(self.store.clone())
                    .execute::<RentalFleetAggregate, _>(metadata, &command)
                    .await
                {
                    Ok(rostfrei::CommandOutcome::Accepted(_)) => CommandOutcome::Accepted,
                    Ok(rostfrei::CommandOutcome::Rejected(rejection)) => {
                        CommandOutcome::Rejected(local_rejection(&rejection)?)
                    }
                    Err(error) => return Err(local_execution_error(error)),
                }
            }
            (ReturnBicycle::COMMAND_NAME, ReturnBicycle::SCHEMA_VERSION) => {
                let command =
                    ReturnBicycle::decode_json(invocation.payload()).map_err(|error| {
                        local_transport_error(CommandTransportErrorKind::InvalidRequest, error)
                    })?;
                match Executor::new(self.store.clone())
                    .execute::<RentalFleetAggregate, _>(metadata, &command)
                    .await
                {
                    Ok(rostfrei::CommandOutcome::Accepted(_)) => CommandOutcome::Accepted,
                    Ok(rostfrei::CommandOutcome::Rejected(rejection)) => {
                        CommandOutcome::Rejected(local_rejection(&rejection)?)
                    }
                    Err(error) => return Err(local_execution_error(error)),
                }
            }
            (AddBicycle::COMMAND_NAME, AddBicycle::SCHEMA_VERSION) => {
                let command = AddBicycle::decode_json(invocation.payload()).map_err(|error| {
                    local_transport_error(CommandTransportErrorKind::InvalidRequest, error)
                })?;
                match Executor::new(self.store.clone())
                    .execute::<RentalFleetAggregate, _>(metadata, &command)
                    .await
                {
                    Ok(rostfrei::CommandOutcome::Accepted(_)) => CommandOutcome::Accepted,
                    Ok(rostfrei::CommandOutcome::Rejected(rejection)) => match rejection {},
                    Err(error) => return Err(local_execution_error(error)),
                }
            }
            _ => {
                return Err(local_transport_error(
                    CommandTransportErrorKind::InvalidRequest,
                    "unexpected command route",
                ));
            }
        };
        let response_message_id =
            ContentFingerprint::digest(format!("local-response:{command_message_id}")).to_hex();
        match outcome {
            CommandOutcome::Accepted => Ok(CommandReceipt::accepted(
                command_message_id,
                response_message_id,
                false,
            )),
            CommandOutcome::Rejected(rejection) => Ok(CommandReceipt::rejected(
                command_message_id,
                response_message_id,
                false,
                rejection,
            )),
        }
    }
}

fn local_rejection<Error>(error: &Error) -> Result<CommandRejection, CommandTransportError>
where
    Error: DomainErrorType + JsonErrorPayload,
{
    let descriptor = Error::DESCRIPTOR;
    Ok(CommandRejection::new(
        "conflict",
        descriptor.code,
        descriptor.message,
        Some(error.encode_json().map_err(|error| {
            local_transport_error(CommandTransportErrorKind::InvalidResponse, error)
        })?),
    ))
}

fn local_execution_error(error: CommandExecutionError) -> CommandTransportError {
    local_transport_error(CommandTransportErrorKind::Unavailable, error.to_string())
}

fn local_transport_error(
    kind: CommandTransportErrorKind,
    message: impl Into<String>,
) -> CommandTransportError {
    CommandTransportError::new(kind, message)
}

async fn fixture() -> (Tracer, ResettableStore, InMemoryEventStore) {
    let test_store = ResettableStore::new();
    seed_demo(&test_store).await.unwrap();
    let production_store = InMemoryEventStore::new();
    seed_demo(&production_store).await.unwrap();
    let history: Arc<dyn EventHistory> = Arc::new(test_store.clone());
    let test_transport: Arc<dyn CommandTransport> =
        Arc::new(LocalCommandTransport::new(test_store.clone()));
    let production_transport: Arc<dyn CommandTransport> =
        Arc::new(LocalCommandTransport::new(production_store.clone()));
    let test_reset: Arc<dyn TestScenarioReset> = Arc::new(test_store.clone());
    let test_repository: Arc<dyn TestRepository> = Arc::new(
        FilesystemTestRepository::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tracer"),
        )
        .unwrap(),
    );
    let mut builder = tracer::builder(history)
        .unwrap()
        .with_domain_model(domain_model().unwrap())
        .with_test_event_store(Arc::new(test_store.clone()))
        .with_test_transport(test_transport)
        .with_dispatch_transport(production_transport)
        .with_stream_directory(Arc::new(test_store.clone()))
        .with_test_fixture("demo-fleet", test_reset)
        .with_test_repository(test_repository)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<RentBicycle>().unwrap();
    builder.register_json::<ReturnBicycle>().unwrap();
    builder.register_json::<AddBicycle>().unwrap();
    builder
        .register_input_options::<RentBicycle, _>(RentBicycleInputOptions)
        .unwrap();
    builder
        .register_input_options::<ReturnBicycle, _>(ReturnBicycleInputOptions)
        .unwrap();
    (builder.build().unwrap(), test_store, production_store)
}

fn app(tracer: Tracer) -> axum::Router {
    http::router(
        tracer,
        HttpConfig::new(API_TOKEN)
            .unwrap()
            .with_dispatch_token(DISPATCH_TOKEN)
            .unwrap(),
    )
}

fn authorize(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header("authorization", format!("Bearer {API_TOKEN}"))
}

fn simulation_request(operation_id: &str, bicycle_id: &str) -> Request<Body> {
    command_request("simulate", operation_id, bicycle_id, API_TOKEN)
}

fn command_request(mode: &str, operation_id: &str, bicycle_id: &str, token: &str) -> Request<Body> {
    operation_request("rent-bicycle", mode, operation_id, bicycle_id, token)
}

fn operation_request(
    command: &str,
    mode: &str,
    operation_id: &str,
    bicycle_id: &str,
    token: &str,
) -> Request<Body> {
    operation_request_with_payload(
        command,
        mode,
        operation_id,
        json!({ "bicycle_id": bicycle_id }),
        token,
    )
}

fn operation_request_with_payload(
    command: &str,
    mode: &str,
    operation_id: &str,
    payload: Value,
    token: &str,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!(
            "/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/{command}/{mode}"
        ))
        .header("content-type", "application/json")
        .header("idempotency-key", operation_id)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            json!({
                "schemaVersion": 1,
                "payload": payload
            })
            .to_string(),
        ))
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn terminal_operation(app: &axum::Router, operation_id: &str, token: &str) -> Value {
    for _ in 0..100 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/operations/{operation_id}"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let operation = json_body(response).await;
        if matches!(
            operation["status"].as_str(),
            Some("completed" | "failed" | "indeterminate")
        ) {
            return operation;
        }
        tokio::task::yield_now().await;
    }
    panic!("operation did not become terminal");
}

fn assert_published_result(operation: &Value) {
    let result = &operation["result"];
    assert_eq!(result["published"], true);
    assert_eq!(result["duplicate"], false);
    assert_eq!(result["baseStreamVersion"], Value::Null);
    assert_eq!(result["appended"], Value::Null);
    assert_eq!(result["commandMessageId"].as_str().unwrap().len(), 64);
    assert_eq!(result["responseMessageId"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn catalog_and_aggregate_instances_are_discovered_through_the_authenticated_api() {
    let (tracer, _, _) = fixture().await;
    let app = app(tracer);

    let unauthorized = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/catalog")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let catalog = json_body(response).await;
    assert_eq!(catalog["catalogVersion"], 3);
    assert_eq!(catalog["testScenario"]["resetHref"], "/test-scenario/reset");
    assert_eq!(catalog["testScenario"]["fixtures"][0], "demo-fleet");
    assert_eq!(catalog["testRepository"]["definitionsHref"], "/tests");
    assert_eq!(catalog["contexts"][0]["id"], "bike-rental");
    assert_eq!(catalog["contexts"][0]["label"], "Bike Rental");
    let aggregate = &catalog["contexts"][0]["aggregates"][0];
    assert_eq!(aggregate["id"], "rental-fleet");
    assert_eq!(aggregate["label"], "Rental fleet");
    assert_eq!(aggregate["aggregateType"], "bike-rental/rental-fleet");
    assert_eq!(
        aggregate["instancesHref"],
        "/contexts/bike-rental/aggregates/rental-fleet/instances"
    );
    let commands = aggregate["commands"].as_array().unwrap();
    assert_eq!(
        commands
            .iter()
            .map(|command| command["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["add-bicycle", "rent-bicycle", "return-bicycle"]
    );
    let add_command = commands
        .iter()
        .find(|command| command["id"] == "add-bicycle")
        .unwrap();
    assert_eq!(add_command["versions"][0]["fields"], json!([]));
    assert_eq!(add_command["versions"][0]["payloadTemplate"], json!({}));
    let command = commands
        .iter()
        .find(|command| command["id"] == "rent-bicycle")
        .unwrap();
    assert_eq!(command["id"], "rent-bicycle");
    assert_eq!(command["label"], "Rent bicycle");
    assert_eq!(command["versions"][0]["schemaVersion"], 1);
    assert_eq!(
        command["versions"][0]["payloadTemplate"],
        json!({ "bicycle_id": "" })
    );
    assert_eq!(
        command["versions"][0]["simulateHrefTemplate"],
        "/contexts/bike-rental/aggregates/rental-fleet/{aggregateId}/commands/rent-bicycle/simulate"
    );
    assert_eq!(
        command["versions"][0]["testHrefTemplate"],
        "/contexts/bike-rental/aggregates/rental-fleet/{aggregateId}/commands/rent-bicycle/test"
    );
    assert_eq!(
        command["versions"][0]["dispatchHrefTemplate"],
        "/contexts/bike-rental/aggregates/rental-fleet/{aggregateId}/commands/rent-bicycle/dispatch"
    );
    assert_eq!(
        command["versions"][0]["inputsHrefTemplate"],
        "/contexts/bike-rental/aggregates/rental-fleet/{aggregateId}/commands/rent-bicycle/schemas/1/inputs"
    );

    let tests = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/tests")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(tests.status(), StatusCode::OK);
    let tests = json_body(tests).await;
    assert_eq!(
        tests["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|test| test["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "reject-unavailable-bicycle",
            "rent-available-bicycle",
            "return-rented-bicycle"
        ]
    );

    let test = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/tests/rent-available-bicycle")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(test.status(), StatusCode::OK);
    let test = json_body(test).await;
    assert_eq!(test["definition"]["given"]["fixture"], "demo-fleet");
    assert_eq!(test["revision"].as_str().unwrap().len(), 64);

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(aggregate["instancesHref"].as_str().unwrap())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "items": [{
                "aggregateId": "city-fleet",
                "streamVersion": 1
            }]
        })
    );

    let inputs_href = command["versions"][0]["inputsHrefTemplate"]
        .as_str()
        .unwrap()
        .replace("{aggregateId}", "city-fleet");
    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(inputs_href)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "fields": [{
                "name": "bicycle_id",
                "label": "Bicycle",
                "options": [{
                    "value": "bike-42",
                    "label": "bike-42",
                    "description": "Available and serviceable"
                }]
            }]
        })
    );

    let inputs_href = add_command["versions"][0]["inputsHrefTemplate"]
        .as_str()
        .unwrap()
        .replace("{aggregateId}", "city-fleet");
    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(inputs_href)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await, json!({ "fields": [] }));

    let return_command = commands
        .iter()
        .find(|command| command["id"] == "return-bicycle")
        .unwrap();
    let inputs_href = return_command["versions"][0]["inputsHrefTemplate"]
        .as_str()
        .unwrap()
        .replace("{aggregateId}", "city-fleet");
    let response = app
        .oneshot(
            authorize(Request::builder())
                .uri(inputs_href)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await,
        json!({
            "fields": [{
                "name": "bicycle_id",
                "label": "Bicycle",
                "options": []
            }]
        })
    );
}

#[tokio::test]
async fn accepted_simulation_streams_a_resumable_trace_without_appending() {
    let (tracer, store, _) = fixture().await;
    let history_before = store.load(&demo_stream()).await.unwrap();
    let app = app(tracer);

    let response = app
        .clone()
        .oneshot(simulation_request("operation-accepted", "bike-42"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/operations/operation-accepted"
    );
    let queued = json_body(response).await;
    assert_eq!(queued["operationId"], "operation-accepted");
    assert_eq!(queued["status"], "queued");

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/operation-accepted/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let trace = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(trace.contains("event: operation.queued"));
    assert!(trace.contains("event: operation.started"));
    assert!(trace.contains("event: history.replayed"));
    assert!(trace.contains("event: command.accepted"));
    assert!(trace.contains("event: domain-event.predicted"));
    assert!(trace.contains("event: operation.completed"));
    assert!(trace.contains("\"eventType\":\"bicycle-rented\""));

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/operation-accepted")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let completed = json_body(response).await;
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["result"]["decision"], "accepted");
    assert_eq!(completed["result"]["baseStreamVersion"], 1);
    assert_eq!(completed["result"]["appended"], false);
    assert_eq!(completed["result"]["published"], false);
    assert_eq!(completed["aggregateType"], "bike-rental/rental-fleet");
    assert_eq!(
        completed["result"]["predictedEvents"][0]["schemaVersion"],
        1
    );
    assert_eq!(
        completed["result"]["predictedEvents"][0]["payload"],
        json!({
            "fleet_id": "city-fleet",
            "bicycle_id": "bike-42",
        })
    );
    let latest = completed["latestEventId"].as_u64().unwrap();

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/operation-accepted/events")
                .header("last-event-id", "2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let resumed = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(!resumed.contains("event: operation.queued"));
    assert!(!resumed.contains("event: operation.started"));
    assert!(resumed.contains("event: history.replayed"));
    assert!(resumed.contains("event: operation.completed"));

    let response = app
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/operation-accepted/events")
                .header("last-event-id", latest.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.load(&demo_stream()).await.unwrap(), history_before);
}

#[tokio::test]
async fn rejection_and_idempotency_have_explicit_http_outcomes() {
    let (tracer, store, _) = fixture().await;
    let history_before = store.load(&demo_stream()).await.unwrap();
    let app = app(tracer);

    let first = app
        .clone()
        .oneshot(simulation_request("operation-rejected", "bike-99"))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::ACCEPTED);

    let repeated = app
        .clone()
        .oneshot(simulation_request("operation-rejected", "bike-99"))
        .await
        .unwrap();
    assert_eq!(repeated.status(), StatusCode::ACCEPTED);

    let conflict = app
        .clone()
        .oneshot(simulation_request("operation-rejected", "bike-42"))
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(conflict).await["code"], "identity-conflict");

    let trace = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/operation-rejected/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let trace = String::from_utf8(trace.to_vec()).unwrap();
    assert!(trace.contains("event: command.rejected"));
    assert!(trace.contains("BICYCLE_UNAVAILABLE"));

    let response = app
        .oneshot(
            authorize(Request::builder())
                .uri("/operations/operation-rejected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let completed = json_body(response).await;
    assert_eq!(completed["result"]["decision"], "rejected");
    assert_eq!(completed["result"]["baseStreamVersion"], 1);
    assert_eq!(completed["result"]["appended"], false);
    assert_eq!(completed["result"]["published"], false);
    assert_eq!(
        completed["result"]["rejection"]["code"],
        "BICYCLE_UNAVAILABLE"
    );
    assert_eq!(store.load(&demo_stream()).await.unwrap(), history_before);
}

#[tokio::test]
async fn transported_http_commands_require_an_idempotency_key() {
    let (tracer, _, _) = fixture().await;
    let app = app(tracer);

    for (mode, token) in [("test", API_TOKEN), ("dispatch", DISPATCH_TOKEN)] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/rent-bicycle/{mode}"
                    ))
                    .header("content-type", "application/json")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::from(
                        json!({
                            "schemaVersion": 1,
                            "payload": { "bicycle_id": "bike-42" }
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_body(response).await["code"],
            "idempotency-key-required"
        );
    }
}

#[tokio::test]
async fn test_is_stateful_simulate_reads_test_history_and_dispatch_is_isolated() {
    let (tracer, test_store, production_store) = fixture().await;
    let app = app(tracer);

    let response = app
        .clone()
        .oneshot(command_request(
            "test",
            "rent-bike-first",
            "bike-42",
            API_TOKEN,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let first_id = json_body(response).await["operationId"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = terminal_operation(&app, &first_id, API_TOKEN).await;
    assert_eq!(first["mode"], "test");
    assert_eq!(first["result"]["decision"], "accepted");
    assert_published_result(&first);
    assert_eq!(first["result"]["predictedEvents"], json!([]));
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 2);
    assert_eq!(
        production_store.load(&demo_stream()).await.unwrap().len(),
        1
    );

    let response = app
        .clone()
        .oneshot(command_request(
            "test",
            "rent-bike-second",
            "bike-42",
            API_TOKEN,
        ))
        .await
        .unwrap();
    let second_id = json_body(response).await["operationId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second = terminal_operation(&app, &second_id, API_TOKEN).await;
    assert_eq!(second["result"]["decision"], "rejected");
    assert_published_result(&second);
    assert_eq!(second["result"]["rejection"]["code"], "BICYCLE_UNAVAILABLE");
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 2);

    let response = app
        .clone()
        .oneshot(simulation_request("simulate-after-test", "bike-42"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let simulated = terminal_operation(&app, "simulate-after-test", API_TOKEN).await;
    assert_eq!(simulated["mode"], "simulate");
    assert_eq!(simulated["result"]["decision"], "rejected");
    assert_eq!(simulated["result"]["baseStreamVersion"], 2);
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 2);

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/return-bicycle/schemas/1/inputs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["fields"][0]["options"][0]["value"],
        "bike-42"
    );

    let response = app
        .clone()
        .oneshot(operation_request(
            "return-bicycle",
            "test",
            "return-bike",
            "bike-42",
            API_TOKEN,
        ))
        .await
        .unwrap();
    let return_id = json_body(response).await["operationId"]
        .as_str()
        .unwrap()
        .to_owned();
    let returned = terminal_operation(&app, &return_id, API_TOKEN).await;
    assert_eq!(returned["result"]["decision"], "accepted");
    assert_published_result(&returned);
    assert_eq!(returned["result"]["predictedEvents"], json!([]));
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 3);
    assert_eq!(
        test_store
            .load(&demo_stream())
            .await
            .unwrap()
            .last()
            .unwrap()
            .event_type(),
        "bicycle-returned"
    );

    let response = app
        .clone()
        .oneshot(operation_request_with_payload(
            "add-bicycle",
            "test",
            "add-bike-77",
            json!({}),
            API_TOKEN,
        ))
        .await
        .unwrap();
    let add_id = json_body(response).await["operationId"]
        .as_str()
        .unwrap()
        .to_owned();
    let added = terminal_operation(&app, &add_id, API_TOKEN).await;
    assert_eq!(added["result"]["decision"], "accepted");
    assert_published_result(&added);
    assert_eq!(added["result"]["predictedEvents"], json!([]));
    let history = test_store.load(&demo_stream()).await.unwrap();
    let added_event = history.last().unwrap();
    assert_eq!(added_event.event_type(), "bicycle-added");
    let added_payload: Value = serde_json::from_slice(added_event.payload()).unwrap();
    let generated_bicycle_id = added_payload["bicycle_id"].as_str().unwrap();
    assert!(Uuid::parse_str(generated_bicycle_id).is_ok());
    let expected_bicycle_id = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        b"rostfrei:bike-rental:bicycle:v1:city-fleet:2",
    );
    assert_eq!(generated_bicycle_id, expected_bicycle_id.to_string());
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 4);

    let response = app
        .clone()
        .oneshot(operation_request_with_payload(
            "add-bicycle",
            "test",
            "add-bike-77",
            json!({}),
            API_TOKEN,
        ))
        .await
        .unwrap();
    assert_eq!(json_body(response).await["operationId"], add_id);
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 4);

    let forbidden = app
        .clone()
        .oneshot(command_request(
            "dispatch",
            "production-rent",
            "bike-42",
            API_TOKEN,
        ))
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let response = app
        .clone()
        .oneshot(command_request(
            "dispatch",
            "production-rent",
            "bike-42",
            DISPATCH_TOKEN,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let dispatch_id = json_body(response).await["operationId"]
        .as_str()
        .unwrap()
        .to_owned();

    let forbidden = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri(format!("/operations/{dispatch_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let dispatched = terminal_operation(&app, &dispatch_id, DISPATCH_TOKEN).await;
    assert_eq!(dispatched["mode"], "dispatch");
    assert_eq!(dispatched["result"]["decision"], "accepted");
    assert_published_result(&dispatched);
    assert_eq!(
        production_store.load(&demo_stream()).await.unwrap().len(),
        2
    );
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 4);

    let unauthorized_reset = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/test-scenario/reset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized_reset.status(), StatusCode::UNAUTHORIZED);

    let reset = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/test-scenario/reset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reset.status(), StatusCode::NO_CONTENT);
    assert_eq!(test_store.load(&demo_stream()).await.unwrap().len(), 1);
    assert_eq!(
        production_store.load(&demo_stream()).await.unwrap().len(),
        2
    );

    let cleared_test_operation = app
        .oneshot(
            authorize(Request::builder())
                .uri(format!("/operations/{add_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cleared_test_operation.status(), StatusCode::NOT_FOUND);
}
