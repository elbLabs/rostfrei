use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use bike_rental::{BikeRentalNatsResourceLimits, BikeRentalNatsRuntime, demo::demo_fixture};
use http_body_util::BodyExt as _;
use rostfrei::DomainRegistry;
use rostfrei_nats::{NatsConnectionConfig, connect};
use rostfrei_tracer::{
    TestScenarioReset, TracerBuilder,
    http::{HttpConfig, router},
};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt as _;

#[allow(clippy::unwrap_used)]
async fn request(app: &Router, method: &str, href: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(href)
                .header("authorization", "Bearer quarantine-test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        },
    )
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn poisoned_test_delivery_is_inspectable_through_catalog_and_cleared_by_reset() {
    let Ok(url) = std::env::var("ROSTFREI_NATS_URL") else {
        return;
    };
    let application = format!("quarantine-api-{}", std::process::id());
    let connection = connect(&NatsConnectionConfig::new("quarantine-api-test", url))
        .await
        .unwrap();
    let runtime = Arc::new(
        BikeRentalNatsRuntime::provision_test_with_resource_limits(
            connection.clone(),
            &application,
            BikeRentalNatsResourceLimits::new(16 * 1024 * 1024, 32 * 1024 * 1024, 512 * 1024),
        )
        .await
        .unwrap(),
    );
    let fixture = demo_fixture().unwrap();
    runtime.reset(&fixture).await.unwrap();
    let store = Arc::new(runtime.store().clone());
    let tracer = TracerBuilder::new(store.clone(), DomainRegistry::new())
        .with_test_event_store(store)
        .with_test_transport(runtime.transport())
        .with_test_scenario_reset(runtime.clone())
        .with_default_test_fixture(fixture)
        .with_test_quarantine_reader(runtime.quarantine_reader())
        .build()
        .unwrap();
    let app = router(tracer, HttpConfig::new("quarantine-test").unwrap());
    let (_, catalog) = request(&app, "GET", "/catalog").await;
    let list_href = catalog["quarantine"]["test"]["listHref"].as_str().unwrap();
    let address = runtime
        .config()
        .context()
        .command_address("rent-bicycle")
        .unwrap();
    connection
        .jetstream()
        .publish(address.as_str().to_owned(), "malformed command".into())
        .await
        .unwrap()
        .await
        .unwrap();
    let list = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (status, list) = request(&app, "GET", list_href).await;
            assert_eq!(status, StatusCode::OK);
            if !list["items"].as_array().unwrap().is_empty() {
                break list;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let detail_href = list["items"][0]["detailHref"].as_str().unwrap();
    let (status, detail) = request(&app, "GET", detail_href).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["failureKind"], "invalid-source-message");
    assert_eq!(detail["reason"], "invalid source message");
    assert_eq!(detail["payload"]["status"], "binary-or-invalid-json");
    assert_eq!(list["retainedMessages"], "1");
    let reset_href = catalog["testScenario"]["resetHref"].as_str().unwrap();
    assert_eq!(
        request(&app, "POST", reset_href).await.0,
        StatusCode::NO_CONTENT
    );
    let (status, body) = request(&app, "GET", detail_href).await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(body["code"], "quarantine-reset");
    assert!(
        request(&app, "GET", list_href).await.1["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    runtime.stop_workers().await;
    for stream in runtime.config().messaging().streams() {
        connection
            .delete_stream_if_exists(stream.name().as_str())
            .await
            .unwrap();
    }
    connection
        .delete_stream_if_exists(runtime.config().event_store().stream_name())
        .await
        .unwrap();
}
