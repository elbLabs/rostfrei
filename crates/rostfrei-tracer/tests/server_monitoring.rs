#![cfg(feature = "nats-host")]
#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions with fallible setup"
)]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    response::Redirect,
    routing::get,
};
use http_body_util::BodyExt as _;
use rostfrei_core::InMemoryEventStore;
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{
    TracerBuilder,
    http::{self, HttpConfig},
    server_monitoring::{NatsServerMonitor, NatsServerMonitorSource, ServerMonitoring},
};
use serde_json::{Value, json};
use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{net::TcpListener, task::JoinHandle};
use tower::ServiceExt as _;

type TestResult = Result<(), Box<dyn Error>>;

struct LocalMonitor(JoinHandle<()>);

impl Drop for LocalMonitor {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn inspection_lists_each_configured_monitor_without_disclosing_targets() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let followed = Arc::new(AtomicBool::new(false));
    let detected = followed.clone();
    let upstream = Router::new()
        .route("/one/healthz", get(|headers: HeaderMap| async move {
            assert!(!headers.contains_key("authorization"));
            r#"{"status":"ok"}"#
        }))
        .route("/one/varz", get(|| async { r#"{"mem":2097152,"uptime":"1h2m3s","version":"2.12.15","host":"private-address","auth_required":true,"jetstream":{"config":{"max_memory":8388608,"store_dir":"/private/path"}}}"# }))
        .route("/two/healthz", get(|| async { (StatusCode::SERVICE_UNAVAILABLE, r#"{"status":"error","error":"private health detail"}"#) }))
        .route("/two/varz", get(move || {
            let location = format!("http://{address}/forbidden");
            async move { Redirect::temporary(&location) }
        }))
        .route("/forbidden", get(move || async move { detected.store(true, Ordering::SeqCst); "secret" }));
    let _guard = LocalMonitor(tokio::spawn(async move {
        let _ = axum::serve(listener, upstream).await;
    }));

    let builder = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), DomainRegistry::new());
    let config = HttpConfig::new("control")?
        .with_dispatch_token("dispatch")?
        .with_inspection_token("inspection")?;
    let disabled = http::router(builder.build()?, config.clone());
    assert!(
        read(&disabled, "/catalog", "inspection")
            .await?
            .1
            .get("servers")
            .is_none()
    );
    assert_eq!(
        read(&disabled, "/inventory/servers", "inspection").await?.0,
        StatusCode::NOT_IMPLEMENTED
    );

    let monitor = NatsServerMonitor::new([
        NatsServerMonitorSource::new("first node", &format!("http://{address}/one"))?,
        NatsServerMonitorSource::new("second node", &format!("http://{address}/two/"))?,
        NatsServerMonitorSource::new("missing node", &format!("http://{address}/missing"))?,
    ])?;
    let app = http::router(
        TracerBuilder::new(Arc::new(InMemoryEventStore::new()), DomainRegistry::new())
            .with_server_monitoring(Arc::new(monitor))
            .build()?,
        config,
    );
    assert!(
        read(&app, "/catalog", "control")
            .await?
            .1
            .get("servers")
            .is_none()
    );
    for token in ["control", "dispatch"] {
        assert_eq!(
            read(&app, "/inventory/servers", token).await?.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        read(&app, "/inventory/servers", "invalid").await?.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        read(&app, "/catalog", "inspection").await?.1["servers"]["snapshotHref"],
        "/inventory/servers"
    );
    let (status, body) = read(&app, "/inventory/servers?target=ignored", "inspection").await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"servers": [
            {"label": "first node", "health": {"status": "available", "value": "healthy"}, "metrics": {"status": "available", "value": {
                "residentMemoryBytes": 2_097_152, "version": "2.12.15", "uptime": "1h2m3s", "jetStreamLimits": {"maxMemoryBytes": 8_388_608}
            }}},
            {"label": "second node", "health": {"status": "available", "value": "unhealthy"}, "metrics": {"status": "unavailable", "reason": "unexpectedHttpStatus"}},
            {"label": "missing node", "health": {"status": "unavailable", "reason": "inaccessible"}, "metrics": {"status": "unavailable", "reason": "inaccessible"}}
        ]})
    );
    assert!(!followed.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn monitoring_bounds_streamed_bodies_and_retains_partial_metrics() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let upstream = Router::new()
        .route("/bounded/healthz", get(|| async {
            let chunks = futures_util::stream::iter([Ok::<_, std::io::Error>(" ".repeat(4097))]);
            Body::from_stream(chunks)
        }))
        .route("/bounded/varz", get(|| async {
            Body::from_stream(futures_util::stream::pending::<Result<String, std::io::Error>>())
        }))
        .route("/partial/healthz", get(|| async { "not a NATS health response" }))
        .route("/partial/varz", get(|| async {
            r#"{"mem":0,"uptime":"https://private-address/path","version":"2.12.15+private.build.address","jetstream":{"config":{"max_storage":0},"stats":{"memory":42000}}}"#
        }));
    let _guard = LocalMonitor(tokio::spawn(async move {
        let _ = axum::serve(listener, upstream).await;
    }));
    let monitor = NatsServerMonitor::new([
        NatsServerMonitorSource::new("bounded", &format!("http://{address}/bounded"))?,
        NatsServerMonitorSource::new("partial", &format!("http://{address}/partial"))?,
    ])?;
    let snapshot =
        tokio::time::timeout(std::time::Duration::from_secs(6), monitor.snapshot()).await?;
    assert_eq!(
        serde_json::to_value(snapshot)?,
        json!({"servers": [
            {"label": "bounded", "health": {"status": "unavailable", "reason": "bodyTooLarge"}, "metrics": {"status": "unavailable", "reason": "timedOut"}},
            {"label": "partial", "health": {"status": "unavailable", "reason": "invalidResponse"}, "metrics": {"status": "available", "value": {
                 "residentMemoryBytes": 0, "version": "2.12.15", "jetStreamLimits": {"maxStorageBytes": 0}
            }}}
        ]})
    );
    Ok(())
}

async fn read(
    app: &Router,
    path: &str,
    token: &str,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let status = response.status();
    Ok((
        status,
        serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?,
    ))
}
