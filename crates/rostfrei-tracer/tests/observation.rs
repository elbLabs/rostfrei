#![cfg(feature = "http")]
#![allow(
    clippy::panic_in_result_fn,
    reason = "test assertions report failures while setup errors use Result"
)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt as _;
use rostfrei_core::InMemoryEventStore;
use rostfrei_messaging_core::ApplicationName;
use rostfrei_registry::DomainRegistry;
use rostfrei_tracer::{
    DomainEventObservation, ExposeTracePayloadsForLocalDevelopment, IntegrationEventObservation,
    ObservationFeed, ObservationScope, ObservationStatus, OperationMode, Tracer, TracerBuilder,
    http::{self, HttpConfig},
};
use serde_json::{Value, json};
use std::{error::Error, sync::Arc, time::Duration};
use tower::ServiceExt as _;

type TestResult = Result<(), Box<dyn Error>>;

fn tracer(expose: bool) -> Result<Tracer, Box<dyn Error>> {
    let application = ApplicationName::new("observed-app")?;
    let mut builder =
        TracerBuilder::new(Arc::new(InMemoryEventStore::new()), DomainRegistry::new())
            .with_continuous_observation(ObservationFeed::new(
                application.clone(),
                ObservationScope::Test,
            ))
            .with_continuous_observation(ObservationFeed::new(
                application,
                ObservationScope::Production,
            ));
    if expose {
        builder =
            builder.with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    }
    Ok(builder.build()?)
}

fn app(tracer: Tracer) -> Result<axum::Router, Box<dyn Error>> {
    Ok(http::router(
        tracer,
        HttpConfig::new("control")?
            .with_dispatch_token("dispatch")?
            .with_inspection_token("inspection")?,
    ))
}

async fn get(
    app: &axum::Router,
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
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    Ok((
        status,
        serde_json::from_slice(&response.into_body().collect().await?.to_bytes())?,
    ))
}

#[tokio::test]
async fn external_events_stream_without_operations_and_remain_scope_isolated() -> TestResult {
    let tracer = tracer(true)?;
    let test = tracer.observation(ObservationScope::Test)?;
    let production = tracer.observation(ObservationScope::Production)?;
    let observer = tracer.correlation_observer(OperationMode::Test);
    let source = test.source("events")?;
    let mut subscription = test.subscribe()?;
    assert_eq!(
        subscription.next().await.ok_or("missing snapshot")?.status,
        ObservationStatus::Connecting
    );
    source.ready();
    assert_eq!(
        subscription
            .next()
            .await
            .ok_or("missing ready snapshot")?
            .status,
        ObservationStatus::Live
    );
    let domain = DomainEventObservation::new("domain-1", "message-received", 1)
        .with_aggregate("inbox/conversation", "conversation-1")
        .with_causation_id("external-command")
        .with_payload(json!({"text": "hello"}));
    observer
        .observe_domain_event("external-flow", domain.clone())
        .await?;
    let snapshot = subscription.next().await.ok_or("missing event snapshot")?;
    drop(subscription);
    assert_eq!(snapshot.items.len(), 1);
    let summary = snapshot.items.first().ok_or("missing external flow")?;
    assert!(production.snapshot().items.is_empty());
    assert!(tracer.correlation_mode("external-flow").is_err());
    let _ = observer.observe_domain_event("external-flow", domain).await;
    assert_eq!(test.snapshot().revision, snapshot.revision);
    let _ = observer
        .observe_integration_event(
            "external-flow",
            IntegrationEventObservation::new(
                "integration-1",
                "message-published",
                1,
                "observed-app.test.integration.inbox.message-published",
            )
            .with_causation_id("domain-1"),
        )
        .await;
    let flow = test.flow(&summary.id)?;
    assert_eq!(flow.message_series.messages().len(), 2);
    assert!(flow.message_series.command_outcomes().is_empty());
    assert!(flow.partial);
    assert_eq!(
        flow.message_series
            .messages()
            .get("integration-1")
            .ok_or("missing integration")?
            .causation_id(),
        Some("domain-1")
    );
    drop(source);
    assert_eq!(test.snapshot().status, ObservationStatus::Unavailable);
    let restarted = test.source("events")?;
    restarted.ready();
    assert_eq!(test.snapshot().status, ObservationStatus::Live);
    Ok(())
}

#[tokio::test]
async fn production_discovery_and_all_resources_require_inspection_capability() -> TestResult {
    let tracer = tracer(false)?;
    let observer = tracer.correlation_observer(OperationMode::Dispatch);
    let _ = observer
        .observe_domain_event(
            "external-production",
            DomainEventObservation::new("sensitive-event", "message-received", 1)
                .with_payload(json!({"secret":"hidden"})),
        )
        .await;
    let feed = tracer.observation(ObservationScope::Production)?;
    let summary = feed
        .snapshot()
        .items
        .into_iter()
        .next()
        .ok_or("missing flow")?;
    let app = app(tracer)?;
    let (_, control_catalog) = get(&app, "/catalog", "control").await?;
    assert_eq!(control_catalog["observation"][0]["scope"], "test");
    assert_eq!(
        control_catalog["observation"]
            .as_array()
            .ok_or("missing capabilities")?
            .len(),
        1
    );
    let (_, inspection_catalog) = get(&app, "/catalog", "inspection").await?;
    assert_eq!(inspection_catalog["observation"][0]["scope"], "production");
    for path in [
        "/observation/production",
        summary.detail_href.as_str(),
        "/observation/production/events",
    ] {
        for token in ["control", "dispatch"] {
            assert_eq!(get(&app, path, token).await?.0, StatusCode::FORBIDDEN);
        }
        assert_eq!(
            get(&app, path, "invalid").await?.0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        get(&app, "/observation/test", "inspection").await?.0,
        StatusCode::FORBIDDEN
    );
    let (status, detail) = get(&app, &summary.detail_href, "inspection").await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        detail["messageSeries"]["messages"][0]
            .get("payload")
            .is_none()
    );
    assert!(!detail.to_string().contains("hidden"));
    Ok(())
}

#[tokio::test]
async fn redaction_preserves_conflict_detection_and_retention_is_bounded() -> TestResult {
    let tracer = tracer(false)?;
    let observer = tracer.correlation_observer(OperationMode::Test);
    let feed = tracer.observation(ObservationScope::Test)?;
    for secret in ["first", "second"] {
        let _ = observer
            .observe_domain_event(
                "conflict",
                DomainEventObservation::new("same-id", "event", 1)
                    .with_payload(json!({"secret":secret})),
            )
            .await;
    }
    let summary = feed
        .snapshot()
        .items
        .into_iter()
        .next()
        .ok_or("missing flow")?;
    assert!(summary.conflicted);
    assert_eq!(summary.message_count, 1);
    assert_eq!(feed.snapshot().discarded_messages, "1");
    for index in 0..129 {
        let _ = observer
            .observe_domain_event(
                &format!("flow-{index}"),
                DomainEventObservation::new(format!("event-{index}"), "event", 1),
            )
            .await;
    }
    assert_eq!(feed.snapshot().items.len(), 128);
    assert_eq!(feed.snapshot().evicted_flows, "2");
    assert!(feed.flow(&summary.id).is_err());
    let app = app(tracer)?;
    assert_eq!(
        get(&app, &summary.detail_href, "control").await?.0,
        StatusCode::GONE
    );
    Ok(())
}

#[tokio::test]
async fn http_stream_starts_with_snapshot_and_cancels_without_pinning_capacity() -> TestResult {
    let tracer = tracer(true)?;
    let feed = tracer.observation(ObservationScope::Test)?;
    let source = feed.source("events")?;
    source.ready();
    let app = app(tracer)?;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/observation/test/events")
                .header("authorization", "Bearer control")
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await?
        .ok_or("missing frame")??;
    let data = std::str::from_utf8(frame.data_ref().ok_or("missing data")?)?;
    assert!(data.contains("event: observation"));
    assert!(data.contains("\"status\":\"live\""));
    drop(body);
    let subscriptions = (0..16)
        .map(|_| feed.subscribe())
        .collect::<Result<Vec<_>, _>>()?;
    assert!(feed.subscribe().is_err());
    drop(subscriptions);
    assert!(feed.subscribe().is_ok());
    Ok(())
}
