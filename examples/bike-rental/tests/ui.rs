use axum::{body::Body, http::Request};
use bike_rental::ui;
use http_body_util::BodyExt as _;
use tower::ServiceExt as _;

#[tokio::test]
async fn example_serves_a_command_ui_for_the_simulation_api() {
    let response = ui::router()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("Bike rental command lab"));
    assert!(body.contains("rent-bicycle"));
    assert!(body.contains("/v1/operations/"));
    assert!(body.contains("last-event-id"));
    assert!(body.contains("operation.failed"));
    assert!(!body.contains("value=\"local-development-token\""));
}
