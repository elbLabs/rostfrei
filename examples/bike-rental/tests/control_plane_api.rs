use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bike_rental::{
    rental::RentBicycle,
    runtime::{RentBicycleWireCodec, control_plane_builder, demo_stream, seed_demo},
};
use http_body_util::BodyExt as _;
use rostfrei::{
    CommandDefinition, DomainModule, DomainRegistry, EventHistory, InMemoryEventStore,
    ModuleDescriptor,
};
use rostfrei_control_plane::{
    ControlPlane, ControlPlaneBuilder, ExposeTracePayloadsForLocalDevelopment,
    MAX_COMMAND_PAYLOAD_LEN, RuntimeRegistrationError, SimulationRequest, SubmissionError,
    http::{self, HttpConfig},
};
use serde_json::{Value, json};
use tower::ServiceExt as _;

const API_TOKEN: &str = "integration-test-capability";

async fn fixture() -> (ControlPlane, InMemoryEventStore) {
    let store = InMemoryEventStore::new();
    seed_demo(&store).await.unwrap();
    let history: Arc<dyn EventHistory> = Arc::new(store.clone());
    let mut builder = control_plane_builder(history)
        .unwrap()
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder
        .register::<RentBicycle, _>(RentBicycleWireCodec)
        .unwrap();
    (builder.build().unwrap(), store)
}

fn app(control_plane: ControlPlane) -> axum::Router {
    http::router(control_plane, HttpConfig::new(API_TOKEN).unwrap())
}

fn authorize(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header("authorization", format!("Bearer {API_TOKEN}"))
}

fn simulation_request(operation_id: &str, bicycle_id: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/bike-rental.rent-bicycle/simulate")
        .header("content-type", "application/json")
        .header("idempotency-key", operation_id)
        .header("authorization", format!("Bearer {API_TOKEN}"))
        .body(Body::from(
            json!({
                "schemaVersion": 1,
                "payload": { "bicycle_id": bicycle_id }
            })
            .to_string(),
        ))
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn accepted_simulation_streams_a_resumable_trace_without_appending() {
    let (control_plane, store) = fixture().await;
    let history_before = store.load(&demo_stream()).await.unwrap();
    let app = app(control_plane);

    let response = app
        .clone()
        .oneshot(simulation_request("operation-accepted", "bike-42"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/v1/operations/operation-accepted"
    );
    let queued = json_body(response).await;
    assert_eq!(queued["operationId"], "operation-accepted");
    assert_eq!(queued["status"], "queued");

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/v1/operations/operation-accepted/events")
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
                .uri("/v1/operations/operation-accepted")
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
    let latest = completed["latestEventId"].as_u64().unwrap();

    let response = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/v1/operations/operation-accepted/events")
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
                .uri("/v1/operations/operation-accepted/events")
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
    let (control_plane, _) = fixture().await;
    let app = app(control_plane);

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

    let trace = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .uri("/v1/operations/operation-rejected/events")
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
                .uri("/v1/operations/operation-rejected")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let completed = json_body(response).await;
    assert_eq!(completed["result"]["decision"], "rejected");
    assert_eq!(
        completed["result"]["rejection"]["code"],
        "BICYCLE_UNAVAILABLE"
    );
}

#[tokio::test]
async fn http_requires_a_bearer_capability_and_reports_invalid_input() {
    let (control_plane, _) = fixture().await;
    let app = app(control_plane);

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

    let malformed = app
        .clone()
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/bike-rental.rent-bicycle/simulate")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(malformed).await["code"], "invalid-json");

    let oversized = app
        .oneshot(
            authorize(Request::builder())
                .method("POST")
                .uri("/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/bike-rental.rent-bicycle/simulate")
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
    let store = InMemoryEventStore::new();
    seed_demo(&store).await.unwrap();
    let history: Arc<dyn EventHistory> = Arc::new(store);
    let mut builder = control_plane_builder(history)
        .unwrap()
        .with_maximum_operations(1);
    builder
        .register::<RentBicycle, _>(RentBicycleWireCodec)
        .unwrap();
    let control_plane = builder.build().unwrap();

    control_plane
        .submit_simulation(
            "bike-rental/rental-fleet",
            "city-fleet",
            "bike-rental.rent-bicycle",
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "bicycle_id": "bike-42" }),
            },
            Some("redacted-accepted"),
        )
        .await
        .unwrap();
    let mut subscription = control_plane
        .subscribe("redacted-accepted", 0)
        .await
        .unwrap();
    while subscription.next().await.is_some() {}
    let accepted =
        serde_json::to_value(control_plane.operation("redacted-accepted").await.unwrap()).unwrap();
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

    control_plane
        .submit_simulation(
            "bike-rental/rental-fleet",
            "city-fleet",
            "bike-rental.rent-bicycle",
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "bicycle_id": "bike-99" }),
            },
            Some("redacted-rejected"),
        )
        .await
        .unwrap();
    assert_eq!(
        control_plane.operation("redacted-accepted").await,
        Err(SubmissionError::NotFound)
    );
    let mut subscription = control_plane
        .subscribe("redacted-rejected", 0)
        .await
        .unwrap();
    while subscription.next().await.is_some() {}
    let rejected =
        serde_json::to_value(control_plane.operation("redacted-rejected").await.unwrap()).unwrap();
    assert_eq!(rejected["result"]["rejection"], json!({ "redacted": true }));

    control_plane
        .submit_simulation(
            "bike-rental/rental-fleet",
            "city-fleet",
            "bike-rental.rent-bicycle",
            SimulationRequest {
                schema_version: 1,
                payload: json!({ "bicycle_id": 42 }),
            },
            Some("redacted-failure"),
        )
        .await
        .unwrap();
    let mut subscription = control_plane
        .subscribe("redacted-failure", 0)
        .await
        .unwrap();
    while subscription.next().await.is_some() {}
    let failure =
        serde_json::to_value(control_plane.operation("redacted-failure").await.unwrap()).unwrap();
    assert_eq!(failure["failure"]["code"], "invalid-command-payload");
    assert_eq!(
        failure["failure"]["message"],
        "simulation failure details are redacted"
    );
}

struct MismatchedBikeRentalModule;

impl DomainModule for MismatchedBikeRentalModule {
    const MODULE_NAME: &'static str = "mismatched-bike-rental";

    fn descriptor() -> ModuleDescriptor {
        let mut command = RentBicycle::descriptor();
        command.aggregate_type = "different-aggregate".to_owned();
        ModuleDescriptor {
            module_name: Self::MODULE_NAME,
            commands: vec![command],
        }
    }
}

#[test]
fn runtime_binding_rejects_a_registry_descriptor_for_a_different_command_contract() {
    let mut registry = DomainRegistry::new();
    registry
        .register_module::<MismatchedBikeRentalModule>()
        .unwrap();
    let history: Arc<dyn EventHistory> = Arc::new(InMemoryEventStore::new());
    let mut builder = ControlPlaneBuilder::new(history, registry);

    assert!(matches!(
        builder.register::<RentBicycle, _>(RentBicycleWireCodec),
        Err(RuntimeRegistrationError::DescriptorMismatch {
            command: "bike-rental.rent-bicycle",
            schema_version: 1,
        })
    ));
}
