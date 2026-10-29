#![allow(
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    reason = "end-to-end assertions retain the host lifecycle scenario"
)]

use std::{error::Error, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{body::Body, http::Request};
use bike_rental::{demo::demo_fixture, tracer};
use http_body_util::BodyExt as _;
use rostfrei::nats::{NatsConnectionConfig, NatsEventStoreConfig, connect};
use rostfrei::tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandTransportObserver,
    NatsApplication, NatsHostError, NatsRuntime, NatsRuntimeConfig, TracerConfig,
    command_execution_fingerprint,
    http::{self, HttpConfig},
};
use rostfrei::{ApplicationName, OperationId};
use serde_json::{Value, json};
use tower::ServiceExt as _;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const FLEET: &str = "/aggregates/bike-rental%2Frental-fleet";

struct Observer;

#[async_trait]
impl CommandTransportObserver for Observer {
    async fn command_published(&self, _: CommandPublication) {}
}

async fn command(runtime: &NatsRuntime, operation: &str, name: &str, payload: Value) -> TestResult {
    let receipt = runtime
        .transport()
        .invoke(
            CommandInvocation::new(
                OperationId::new(operation)?,
                operation,
                command_execution_fingerprint("bike-rental", name, 2, &payload),
                "bike-rental",
                name,
                2,
                payload,
            ),
            Arc::new(Observer),
        )
        .await?;
    assert!(matches!(receipt.outcome(), CommandOutcome::Accepted));
    Ok(())
}

async fn get(app: &axum::Router, path: &str, token: &str) -> TestResult<(u16, Value)> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// Integration publication is asynchronous; there is no index to catch up.
async fn published_chain(app: &axum::Router, id: &str, count: usize) -> TestResult<Value> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let (status, body) =
                get(app, &format!("/correlations/{id}/events"), "inspection").await?;
            if status == 200
                && body
                    .get("refs")
                    .and_then(Value::as_array)
                    .is_some_and(|refs| refs.len() == count)
            {
                return TestResult::Ok(body);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?
}

fn correlation_ids(body: &Value) -> Vec<&str> {
    body["correlations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|chain| chain["correlation_id"].as_str())
        .collect()
}

#[tokio::test]
async fn ordinary_runtime_events_are_inspectable_without_a_projector_even_after_shutdown()
-> TestResult {
    inspect_retained_events(false).await
}

#[tokio::test]
async fn inspection_only_host_reads_external_runtime_without_dispatch() -> TestResult {
    inspect_retained_events(true).await
}

#[tokio::test]
async fn production_inspection_rejects_wrong_scope_application_and_conflicting_sources()
-> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "subject-correlations-scope",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let app = ApplicationName::new(format!("subject-scope-{suffix}"))?;
    let normal = NatsEventStoreConfig::for_bounded_context(&app.bounded_context("bike-rental")?)?;
    let wrong_scope =
        NatsEventStoreConfig::for_bounded_context(&app.test_bounded_context("bike-rental")?)?;
    let other_app = ApplicationName::new(format!("other-subject-scope-{suffix}"))?;
    let wrong_app =
        NatsEventStoreConfig::for_bounded_context(&other_app.bounded_context("bike-rental")?)?;
    let conflicting = normal
        .clone()
        .with_fresh_split_outcome_epoch("another-source")?;
    for (sources, expected) in [
        (vec![wrong_scope], "Normal scope"),
        (vec![wrong_app], "host application"),
        (
            vec![normal, conflicting],
            "conflicting production inspection sources",
        ),
    ] {
        let mut config = TracerConfig::new(
            tokio::net::TcpListener::bind("127.0.0.1:0").await?,
            HttpConfig::inspection_only("inspection")?,
        );
        for source in sources {
            config = config.with_production_event_store(source);
        }
        let result = NatsApplication::builder(
            app.test_bounded_context("bike-rental")?,
            connection.clone(),
            Arc::new(tracer::bindings()?),
        )?
        .with_tracer(Some(config))
        .provision_and_start()
        .await;
        assert!(
            matches!(result, Err(NatsHostError::Configuration(message)) if message.contains(expected))
        );
    }
    connection.drain().await?;
    Ok(())
}

async fn inspect_retained_events(inspection_only: bool) -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "subject-correlations-host",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("subject-correlations-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let normal = Arc::new(
        NatsRuntime::provision(
            connection.clone(),
            NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
                .with_fresh_completed_outcome_epoch("subject-correlations")?,
            bindings.clone(),
        )
        .await?,
    );
    normal.apply_fixture(&demo_fixture()?).await?;
    let http = if inspection_only {
        HttpConfig::inspection_only("inspection")?
    } else {
        HttpConfig::new("control")?
            .with_dispatch_token("dispatch")?
            .with_inspection_token("inspection")?
    };
    let config = TracerConfig::new(
        tokio::net::TcpListener::bind("127.0.0.1:0").await?,
        http.clone(),
    )
    .with_trace_payload_policy(Arc::new(
        rostfrei::tracer::ExposeTracePayloadsForLocalDevelopment,
    ));
    let config = if inspection_only {
        normal.start_workers().await?;
        config
            .with_production_event_store(normal.config().event_store.clone())
            .with_production_event_store(normal.config().event_store.clone())
    } else {
        config.with_dispatch_runtime(normal.clone())
    };
    let host = NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("subject-correlations")?
    .with_default_test_fixture(demo_fixture()?)
    .with_tracer(Some(config))
    .provision_and_start()
    .await?;
    let app = http::router(host.tracer().ok_or("missing Tracer")?.clone(), http);

    let (status, catalog) = get(&app, "/catalog", "inspection").await?;
    assert_eq!(status, 200);
    assert_eq!(
        catalog.pointer("/correlations/anchors"),
        Some(&json!([{
            "aggregateType": "bike-rental/rental-fleet",
            "label": "Rental fleet",
            "correlationsHrefTemplate": format!("{FLEET}/{{aggregateId}}/correlations")
        }]))
    );

    command(
        &normal,
        "external-rent",
        "rent-bicycle",
        json!({
            "fleet_id": "city-fleet", "bicycle_id": "bike-42"
        }),
    )
    .await?;
    let rent = published_chain(&app, "external-rent", 2).await?;
    let refs = rent
        .get("refs")
        .and_then(Value::as_array)
        .ok_or("missing refs")?;
    let domain = refs
        .iter()
        .find(|item| item["kind"] == "domain")
        .ok_or("missing domain event")?;
    let integration = refs
        .iter()
        .find(|item| item["kind"] == "integration")
        .ok_or("missing integration event")?;
    assert_eq!(domain["event_type"], "bicycle-rented");
    assert_eq!(integration["causation_id"], domain["event_id"]);
    assert!(
        integration["subject"]
            .as_str()
            .is_some_and(|subject| subject.contains(".external-rent."))
    );
    assert!(rent.get("lag").is_none());
    assert_eq!(
        rent.pointer("/snapshot/sources")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    command(
        &normal,
        "city-bike-7",
        "add-bicycle",
        json!({
            "fleet_id": "city-fleet", "bicycle_id": "bike-7", "condition": "serviceable"
        }),
    )
    .await?;
    command(
        &normal,
        "harbor-bike-8",
        "add-bicycle",
        json!({
            "fleet_id": "harbor-fleet", "bicycle_id": "bike-8", "condition": "serviceable"
        }),
    )
    .await?;
    command(
        &normal,
        "transfer-bike-7",
        "transfer-bicycle",
        json!({
            "from_fleet_id": "city-fleet", "bicycle_id": "bike-7", "to_fleet_id": "harbor-fleet"
        }),
    )
    .await?;

    let (status, harbor) = get(
        &app,
        &format!("{FLEET}/harbor-fleet/correlations?include=context:bike-rental"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "{harbor}");
    assert_eq!(
        correlation_ids(&harbor),
        ["harbor-bike-8", "transfer-bike-7"]
    );
    let transfer = harbor
        .pointer("/correlations/1/refs")
        .and_then(Value::as_array)
        .ok_or("missing transfer")?;
    let source = transfer
        .iter()
        .find(|item| item["aggregate_id"] == "city-fleet")
        .ok_or("missing source")?;
    assert_eq!(
        source
            .pointer("/payload/bicycle_id")
            .and_then(Value::as_str),
        Some("bike-7")
    );

    let (status, matched) = get(
        &app,
        &format!(
            "{FLEET}/city-fleet/correlations?match=bike-rental/bicycle-added.bicycle_id=bike-7"
        ),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "{matched}");
    assert_eq!(correlation_ids(&matched), ["city-bike-7"]);
    let (status, unknown) =
        get(&app, &format!("{FLEET}/bike-42/correlations"), "inspection").await?;
    assert_eq!(status, 404);
    assert_eq!(
        unknown.get("code").and_then(Value::as_str),
        Some("unknown-anchor-stream")
    );
    for token in ["control", "dispatch"] {
        assert_eq!(
            get(&app, &format!("{FLEET}/city-fleet/correlations"), token)
                .await?
                .0,
            if inspection_only { 401 } else { 403 }
        );
    }

    host.shutdown().await?;
    normal.stop_workers().await;
    let (status, after_shutdown) =
        get(&app, "/correlations/external-rent/events", "inspection").await?;
    assert_eq!(
        status, 200,
        "retained reads require no running workers: {after_shutdown}"
    );
    assert_eq!(after_shutdown.get("refs"), rent.get("refs"));
    connection.drain().await?;
    Ok(())
}
