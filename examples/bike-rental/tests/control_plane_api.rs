use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use bike_rental::{
    rental::RentBicycle,
    runtime::{control_plane_builder, demo_stream, seed_demo},
};
use http_body_util::BodyExt as _;
use rostfrei::{EventHistory, InMemoryEventStore};
use rostfrei_control_plane::{
    ControlPlane, ExposeTracePayloadsForLocalDevelopment,
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
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<RentBicycle>().unwrap();
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
        .uri("/v1/contexts/bike-rental/aggregates/rental-fleet/city-fleet/commands/rent-bicycle/simulate")
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
#[allow(clippy::too_many_lines)]
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
    let (control_plane, store) = fixture().await;
    let history_before = store.load(&demo_stream()).await.unwrap();
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
    assert_eq!(json_body(conflict).await["code"], "identity-conflict");

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
    assert_eq!(completed["result"]["baseStreamVersion"], 1);
    assert_eq!(completed["result"]["appended"], false);
    assert_eq!(completed["result"]["published"], false);
    assert_eq!(
        completed["result"]["rejection"]["code"],
        "BICYCLE_UNAVAILABLE"
    );
    assert_eq!(store.load(&demo_stream()).await.unwrap(), history_before);
}
