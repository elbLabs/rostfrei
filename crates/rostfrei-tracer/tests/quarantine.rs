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
    let subject_scope = if scope == TrafficScope::Test {
        ".test"
    } else {
        ""
    };
    let stream_scope = if scope == TrafficScope::Test {
        "__TEST"
    } else {
        ""
    };
    let mut metadata = CallerMetadata::new();
    metadata.insert("x-customer", "sensitive-metadata").unwrap();
    Arc::new(Reader {
        application: ApplicationName::new("shop").unwrap(),
        scope,
        calls: AtomicUsize::new(0),
        entry: QuarantineEntry {
            id: "record/1?x".to_owned(),
            stored_at: "2026-09-10T12:00:00Z".to_owned(),
            subject: format!("shop{subject_scope}.quarantine.command.orders.place"),
            message: Some(QuarantinedMessage {
                message_id: "original-message".to_owned(),
                address: format!("shop{subject_scope}.command.orders.place"),
                payload_base64: STANDARD.encode(br#"{"secret":"sensitive-payload"}"#),
                payload_size: Some(30),
                payload_sha256: Some("digest".to_owned()),
                payload_truncated: false,
                metadata,
                trace_context: Some(
                    rostfrei_messaging_core::TraceContext::from_parts(
                        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                        None::<&str>,
                    )
                    .unwrap(),
                ),
                correlation_id: Some(CorrelationId::new("correlation-1").unwrap()),
                reason: "recorded failure".to_owned(),
                failure_kind: Some(QuarantineFailureKind::HandlerFailure),
                attempt: 2,
                pending: 0,
                source_sequence: 9_007_199_254_740_993,
                consumer_sequence: u64::MAX,
                source_stream: format!("SHOP{stream_scope}_COMMANDS"),
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

#[tokio::test]
async fn production_inspection_has_its_own_discovery_and_cannot_execute_commands_or_reset() {
    let test = reader(TrafficScope::Test);
    let production = reader(TrafficScope::Normal);
    let tracer = builder()
        .with_test_quarantine_reader(test.clone())
        .with_production_quarantine_reader(production.clone())
        .build()
        .unwrap();
    let config = HttpConfig::new("control")
        .unwrap()
        .with_dispatch_token("dispatch")
        .unwrap()
        .with_inspection_token("inspect")
        .unwrap();
    let app = router(tracer, config);
    let (_, control_catalog) = get(&app, "/catalog", "control").await;
    assert!(control_catalog["quarantine"].get("test").is_some());
    assert!(control_catalog["quarantine"].get("production").is_none());
    let (_, catalog) = get(&app, "/catalog", "inspect").await;
    assert_eq!(catalog["contexts"], json!([]));
    assert!(catalog.get("testScenario").is_none());
    assert!(catalog["quarantine"].get("test").is_none());
    let href = catalog["quarantine"]["production"]["listHref"]
        .as_str()
        .unwrap();
    assert_eq!(get(&app, href, "inspect").await.0, StatusCode::OK);
    for token in ["control", "dispatch"] {
        for path in [href, "/quarantine/production/record"] {
            assert_eq!(get(&app, path, token).await.0, StatusCode::FORBIDDEN);
        }
    }
    assert_eq!(
        get(&app, "/quarantine/test", "inspect").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&app, "/quarantine/test/record", "inspect").await.0,
        StatusCode::FORBIDDEN
    );
    for path in [
        "/test-scenario/reset",
        "/contexts/orders/commands/place/dispatch",
        "/contexts/orders/commands/place/test",
        "/contexts/orders/commands/place/simulate",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("authorization", "Bearer inspect")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(test.calls.load(Ordering::Relaxed), 0);
    assert_eq!(production.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn production_payload_policy_is_enforced_in_the_service_and_independent_of_trace_policy() {
    use rostfrei_tracer::{ExposeTracePayloadsForLocalDevelopment, QuarantineScope};
    let tracer = builder()
        .with_production_quarantine_reader(reader(TrafficScope::Normal))
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment))
        .build()
        .unwrap();
    let direct = serde_json::to_value(
        tracer
            .quarantine_message(QuarantineScope::Production, "record")
            .await
            .unwrap(),
    )
    .unwrap();
    let app = router(tracer, HttpConfig::inspection_only("inspect").unwrap());
    let (status, detail) = get(&app, "/quarantine/production/record", "inspect").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail, direct);
    assert_eq!(detail["payload"]["status"], "redacted");
    for field in ["base64", "json", "sha256"] {
        assert!(detail["payload"].get(field).is_none());
    }
    for field in ["metadata", "traceContext", "reason"] {
        assert!(detail.get(field).is_none());
    }
    assert_eq!(detail["failureKind"], "handler-failure");
    let (_, list) = get(&app, "/quarantine/production", "inspect").await;
    assert!(list["items"][0].get("reason").is_none());
    assert!(!detail.to_string().contains("sensitive"));
    assert_eq!(
        get(&app, "/quarantine/production", "control").await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn malformed_production_record_bytes_are_redacted_too() {
    let mut reader = reader(TrafficScope::Normal);
    let fixture = Arc::get_mut(&mut reader).unwrap();
    fixture.entry.message = None;
    fixture.entry.diagnostics = vec![QuarantineDiagnostic::InvalidRecord];
    fixture.entry.invalid_record_base64 =
        Some(STANDARD.encode(br#"{"secret":"sensitive-record"}"#));
    let tracer = builder()
        .with_production_quarantine_reader(reader)
        .build()
        .unwrap();
    let app = router(tracer, HttpConfig::inspection_only("inspect").unwrap());
    let (_, detail) = get(&app, "/quarantine/production/record", "inspect").await;
    assert_eq!(detail["payload"]["content"], "quarantine-record");
    assert_eq!(detail["payload"]["status"], "redacted");
    assert!(detail["payload"].get("base64").is_none());
    assert!(detail["payload"].get("json").is_none());
    assert_eq!(detail["diagnostics"], json!(["invalid-record"]));
}

struct SanitizedProduction;

impl rostfrei_tracer::QuarantinePayloadPolicy for SanitizedProduction {
    fn payload(
        &self,
        scope: rostfrei_tracer::QuarantineScope,
        payload: rostfrei_tracer::QuarantinePayload,
    ) -> rostfrei_tracer::QuarantinePayload {
        if scope == rostfrei_tracer::QuarantineScope::Test {
            return payload;
        }
        let mut sanitized = payload.redacted();
        sanitized.json = Some(json!({"safe": true}));
        sanitized
    }

    fn reason(&self, _scope: rostfrei_tracer::QuarantineScope, _reason: String) -> Option<String> {
        Some("sanitized failure".to_owned())
    }
}

#[tokio::test]
async fn a_custom_policy_can_expose_sanitized_json_without_raw_content_bypasses() {
    let tracer = builder()
        .with_test_quarantine_reader(reader(TrafficScope::Test))
        .with_production_quarantine_reader(reader(TrafficScope::Normal))
        .with_quarantine_payload_policy(Arc::new(SanitizedProduction))
        .build()
        .unwrap();
    let app = router(
        tracer,
        HttpConfig::new("control")
            .unwrap()
            .with_inspection_token("inspect")
            .unwrap(),
    );
    let (_, detail) = get(&app, "/quarantine/production/record", "inspect").await;
    assert_eq!(detail["payload"]["json"], json!({"safe": true}));
    assert_eq!(detail["reason"], "sanitized failure");
    assert!(detail["payload"].get("base64").is_none());
    assert!(detail["payload"].get("sha256").is_none());
    assert!(detail.get("metadata").is_none());
    assert!(!detail.to_string().contains("sensitive"));
    let (_, list) = get(&app, "/quarantine/production", "inspect").await;
    assert_eq!(list["items"][0]["reason"], "sanitized failure");
    let (_, test) = get(&app, "/quarantine/test/record", "control").await;
    assert_eq!(test["payload"]["json"]["secret"], "sensitive-payload");
}

#[test]
fn inspection_configuration_rejects_token_aliases_and_reader_scope_mismatches() {
    use rostfrei_tracer::http::HttpConfigError;
    assert!(matches!(
        HttpConfig::new("same")
            .unwrap()
            .with_inspection_token("same"),
        Err(HttpConfigError::DuplicateBearerTokens)
    ));
    assert!(matches!(
        HttpConfig::new("control")
            .unwrap()
            .with_dispatch_token("same")
            .unwrap()
            .with_inspection_token("same"),
        Err(HttpConfigError::DuplicateBearerTokens)
    ));
    assert!(matches!(
        HttpConfig::inspection_only("same")
            .unwrap()
            .with_dispatch_token("same"),
        Err(HttpConfigError::DuplicateBearerTokens)
    ));
    assert!(HttpConfig::inspection_only("").is_err());
    assert!(matches!(
        builder()
            .with_production_quarantine_reader(reader(TrafficScope::Test))
            .build(),
        Err(RuntimeRegistrationError::InvalidQuarantineScope {
            scope: "production"
        })
    ));
    let mut different = reader(TrafficScope::Normal);
    Arc::get_mut(&mut different).unwrap().application = ApplicationName::new("different").unwrap();
    assert!(matches!(
        builder()
            .with_test_quarantine_reader(reader(TrafficScope::Test))
            .with_production_quarantine_reader(different)
            .build(),
        Err(RuntimeRegistrationError::QuarantineApplicationMismatch)
    ));
}

struct WaitingReader {
    reader: Arc<Reader>,
    started: tokio::sync::Semaphore,
    release: tokio::sync::Notify,
}

#[async_trait]
impl QuarantineReader for WaitingReader {
    fn application(&self) -> &ApplicationName {
        self.reader.application()
    }
    fn traffic_scope(&self) -> TrafficScope {
        self.reader.traffic_scope()
    }
    async fn list(&self, query: QuarantineQuery) -> Result<QuarantinePage, QuarantineReadError> {
        let release = self.release.notified();
        tokio::pin!(release);
        release.as_mut().enable();
        self.started.add_permits(1);
        release.await;
        self.reader.list(query).await
    }
    async fn get(&self, id: &str) -> Result<QuarantineEntry, QuarantineReadError> {
        self.reader.get(id).await
    }
}

#[tokio::test]
async fn test_inspection_load_cannot_exhaust_production_inspection_capacity() {
    use rostfrei_tracer::{QuarantineInspectionError, QuarantineScope};
    let blocked = Arc::new(WaitingReader {
        reader: reader(TrafficScope::Test),
        started: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Notify::new(),
    });
    let tracer = builder()
        .with_test_quarantine_reader(blocked.clone())
        .with_production_quarantine_reader(reader(TrafficScope::Normal))
        .build()
        .unwrap();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let tracer = tracer.clone();
        tasks.push(tokio::spawn(async move {
            tracer
                .quarantine_messages(QuarantineScope::Test, QuarantineQuery::default())
                .await
        }));
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        blocked.started.acquire_many(8),
    )
    .await
    .unwrap()
    .unwrap()
    .forget();
    assert_eq!(
        tracer
            .quarantine_messages(QuarantineScope::Test, QuarantineQuery::default())
            .await,
        Err(QuarantineInspectionError::CapacityExhausted)
    );
    assert!(
        tracer
            .quarantine_messages(QuarantineScope::Production, QuarantineQuery::default())
            .await
            .is_ok()
    );
    blocked.release.notify_waiters();
    for task in tasks {
        task.await.unwrap().unwrap();
    }
}
