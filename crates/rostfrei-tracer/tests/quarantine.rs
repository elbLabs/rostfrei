#![cfg(feature = "http")]

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::BodyExt as _;
use rostfrei_core::InMemoryEventStore;
use rostfrei_messaging_core::{
    ApplicationName, CallerMetadata, CorrelationId, QuarantineDiagnostic, QuarantineEntry,
    QuarantineFailureKind, QuarantinePage, QuarantineQuery, QuarantineReadError, QuarantineReader,
    QuarantinedMessage, TrafficScope,
};
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{
    RuntimeRegistrationError, TracerBuilder,
    http::{HttpConfig, router},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt as _;

struct Reader {
    application: ApplicationName,
    scope: TrafficScope,
    calls: AtomicUsize,
    entry: QuarantineEntry,
}

#[async_trait]
impl QuarantineReader for Reader {
    fn application(&self) -> &ApplicationName {
        &self.application
    }
    fn traffic_scope(&self) -> TrafficScope {
        self.scope
    }
    async fn list(&self, _query: QuarantineQuery) -> Result<QuarantinePage, QuarantineReadError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(QuarantinePage {
            items: vec![self.entry.clone()],
            next_cursor: Some("next/%?".to_owned()),
            retained_messages: 9_007_199_254_740_993,
            snapshot_sequence: u64::MAX,
        })
    }
    async fn get(&self, id: &str) -> Result<QuarantineEntry, QuarantineReadError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        match id {
            "reset" => Err(QuarantineReadError::StaleGeneration),
            "expired" => Err(QuarantineReadError::NotFound),
            "offline" => Err(QuarantineReadError::Unavailable),
            "timeout" => Err(QuarantineReadError::Timeout),
            _ => Ok(self.entry.clone()),
        }
    }
}

#[allow(clippy::unwrap_used)]
fn reader(scope: TrafficScope) -> Arc<Reader> {
    let mut metadata = CallerMetadata::new();
    metadata.insert("x-customer", "sensitive-metadata").unwrap();
    Arc::new(Reader {
        application: ApplicationName::new("shop").unwrap(),
        scope,
        calls: AtomicUsize::new(0),
        entry: QuarantineEntry {
            id: "record/1?x".to_owned(),
            stored_at: "2026-09-10T12:00:00Z".to_owned(),
            subject: "shop.test.quarantine.command.orders.place".to_owned(),
            message: Some(QuarantinedMessage {
                message_id: "original-message".to_owned(),
                address: "shop.test.command.orders.place".to_owned(),
                payload_base64: STANDARD.encode(br#"{"secret":"sensitive-payload"}"#),
                payload_size: Some(30),
                payload_sha256: Some("digest".to_owned()),
                payload_truncated: false,
                metadata,
                trace_context: None,
                correlation_id: Some(CorrelationId::new("correlation-1").unwrap()),
                reason: "recorded failure".to_owned(),
                failure_kind: Some(QuarantineFailureKind::HandlerFailure),
                attempt: 2,
                pending: 0,
                source_sequence: 9_007_199_254_740_993,
                consumer_sequence: u64::MAX,
                source_stream: "SHOP__TEST_COMMANDS".to_owned(),
                source_consumer: "orders-worker".to_owned(),
            }),
            diagnostics: Vec::new(),
            invalid_record_base64: None,
            invalid_record_truncated: false,
        },
    })
}

fn builder() -> TracerBuilder {
    TracerBuilder::new(Arc::new(InMemoryEventStore::new()), DomainRegistry::new())
}

#[allow(clippy::unwrap_used)]
fn app(reader: Arc<Reader>) -> Router {
    router(
        builder()
            .with_test_quarantine_reader(reader)
            .build()
            .unwrap(),
        HttpConfig::new("control")
            .unwrap()
            .with_dispatch_token("dispatch")
            .unwrap(),
    )
}

#[allow(clippy::unwrap_used)]
async fn get(app: &Router, path: &str, token: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn catalog_list_and_detail_expose_test_evidence_and_safe_navigation() {
    let reader = reader(TrafficScope::Test);
    let app = app(reader);
    let (status, catalog) = get(&app, "/catalog", "control").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(catalog["quarantine"]["test"]["application"], "shop");
    let href = catalog["quarantine"]["test"]["listHref"].as_str().unwrap();
    let (_, list) = get(
        &app,
        &format!("{href}?kind=command&context=orders&name=place&limit=1"),
        "control",
    )
    .await;
    assert_eq!(list["scope"], "test");
    assert_eq!(list["retainedMessages"], "9007199254740993");
    assert_eq!(list["snapshotSequence"], u64::MAX.to_string());
    assert_eq!(list["items"][0]["correlationId"], "correlation-1");
    assert!(list["items"][0].get("payload").is_none());
    assert!(list["items"][0].get("metadata").is_none());
    assert!(!list.to_string().contains("sensitive-payload"));
    let next = list["nextHref"].as_str().unwrap();
    assert!(next.contains("cursor=next%2F%25%3F"));
    assert!(next.contains("kind=command&context=orders&name=place"));
    assert_eq!(get(&app, next, "control").await.0, StatusCode::OK);
    let detail_href = list["items"][0]["detailHref"].as_str().unwrap();
    assert_eq!(detail_href, "/quarantine/test/record%2F1%3Fx");
    let (status, detail) = get(&app, detail_href, "control").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["payload"]["status"], "json");
    assert_eq!(detail["payload"]["json"]["secret"], "sensitive-payload");
    assert_eq!(detail["metadata"]["x-customer"], "sensitive-metadata");
    assert_eq!(detail["source"]["sourceSequence"], "9007199254740993");
    assert_eq!(detail["source"]["consumerSequence"], u64::MAX.to_string());
}

#[tokio::test]
async fn authorization_and_invalid_queries_do_not_reach_the_reader() {
    let reader = reader(TrafficScope::Test);
    let app = app(reader.clone());
    for token in ["invalid", "dispatch"] {
        for path in ["/quarantine/test", "/quarantine/test/record"] {
            assert_eq!(get(&app, path, token).await.0, StatusCode::UNAUTHORIZED);
        }
    }
    for query in [
        "limit=0",
        "limit=101",
        "kind=domain-event",
        "context=*",
        "name=a.b",
        "unknown=yes",
        "limit=no",
        "limit=1&limit=2",
    ] {
        assert_eq!(
            get(&app, &format!("/quarantine/test?{query}"), "control")
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(reader.calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn errors_distinguish_reset_expiry_and_unavailability() {
    let app = app(reader(TrafficScope::Test));
    for (id, status, code) in [
        ("reset", StatusCode::GONE, "quarantine-reset"),
        ("expired", StatusCode::NOT_FOUND, "quarantine-not-found"),
        (
            "offline",
            StatusCode::SERVICE_UNAVAILABLE,
            "quarantine-unavailable",
        ),
        ("timeout", StatusCode::GATEWAY_TIMEOUT, "quarantine-timeout"),
    ] {
        let (actual, body) = get(&app, &format!("/quarantine/test/{id}"), "control").await;
        assert_eq!(actual, status);
        assert_eq!(body["code"], code);
    }
}

#[tokio::test]
async fn malformed_and_truncated_evidence_is_not_presented_as_complete_json() {
    let mut reader = reader(TrafficScope::Test);
    let fixture = Arc::get_mut(&mut reader).unwrap();
    fixture.entry.message.as_mut().unwrap().payload_truncated = true;
    let (_, detail) = get(&app(reader), "/quarantine/test/record", "control").await;
    assert_eq!(detail["payload"]["status"], "truncated");
    assert!(detail["payload"].get("json").is_none());
    assert!(detail["payload"].get("base64").is_some());

    let mut reader = self::reader(TrafficScope::Test);
    let fixture = Arc::get_mut(&mut reader).unwrap();
    fixture.entry.message = None;
    fixture.entry.diagnostics = vec![QuarantineDiagnostic::InvalidRecord];
    fixture.entry.invalid_record_base64 = Some(STANDARD.encode(b"malformed record"));
    let (_, detail) = get(&app(reader), "/quarantine/test/record", "control").await;
    assert_eq!(detail["diagnostics"], json!(["invalid-record"]));
    assert_eq!(detail["payload"]["content"], "quarantine-record");
    assert_eq!(detail["payload"]["status"], "binary-or-invalid-json");
    assert!(detail.get("messageId").is_none());
}

#[tokio::test]
async fn missing_capability_is_not_advertised_and_wrong_scope_fails_build() {
    let app = router(
        builder().build().unwrap(),
        HttpConfig::new("control").unwrap(),
    );
    assert!(
        get(&app, "/catalog", "control")
            .await
            .1
            .get("quarantine")
            .is_none()
    );
    assert_eq!(
        get(&app, "/quarantine/test", "control").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(matches!(
        builder()
            .with_test_quarantine_reader(reader(TrafficScope::Normal))
            .build(),
        Err(RuntimeRegistrationError::InvalidQuarantineScope { scope: "test" })
    ));
}
