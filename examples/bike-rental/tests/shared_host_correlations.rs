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
use rostfrei::{
    ApplicationName, CorrelationId, CorrelationIndexReader, CorrelationSourceState, OperationId,
};
use rostfrei_nats::{NatsConnectionConfig, connect};
use rostfrei_tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandTransportObserver,
    NatsApplication, NatsRuntime, NatsRuntimeConfig, TracerConfig, command_execution_fingerprint,
    http::{self, HttpConfig},
};
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
    assert!(
        matches!(receipt.outcome(), CommandOutcome::Accepted),
        "{name} was not accepted: {:?}",
        receipt.outcome()
    );
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
    let bytes = response.into_body().collect().await?.to_bytes();
    Ok((
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    ))
}

async fn indexed(app: &axum::Router, correlation: &str, refs: usize) -> TestResult<Value> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let (status, body) = get(
                app,
                &format!("/correlations/{correlation}/events"),
                "inspection",
            )
            .await?;
            if status == 200
                && body
                    .get("refs")
                    .and_then(Value::as_array)
                    .is_some_and(|items| items.len() == refs)
                && body.pointer("/lag/bike-rental") == Some(&json!(0))
            {
                return TestResult::Ok(body);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?
}

fn correlation_ids(view: &Value) -> Vec<&str> {
    view.get("correlations")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["correlation_id"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn production_correlations_are_indexed_and_viewed_from_their_anchors() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "shared-correlations",
        rostfrei_testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("shared-correlations-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let normal = Arc::new(
        NatsRuntime::provision(
            connection.clone(),
            NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
                .with_fresh_completed_outcome_epoch("correlations")?
                .with_correlation_index()?,
            bindings.clone(),
        )
        .await?,
    );
    normal.apply_fixture(&demo_fixture()?).await?;
    let http = HttpConfig::new("control")?
        .with_dispatch_token("dispatch")?
        .with_inspection_token("inspection")?;
    let config = TracerConfig::new(
        tokio::net::TcpListener::bind("127.0.0.1:0").await?,
        http.clone(),
    )
    .with_dispatch_runtime(normal.clone())
    .with_trace_payload_policy(Arc::new(
        rostfrei_tracer::ExposeTracePayloadsForLocalDevelopment,
    ));
    let host = NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("correlations")?
    .with_default_test_fixture(demo_fixture()?)
    .with_tracer(Some(config))
    .provision_and_start()
    .await?;
    let app = http::router(host.tracer().ok_or("missing Tracer")?.clone(), http);

    let (_, catalog) = get(&app, "/catalog", "inspection").await?;
    assert_eq!(
        catalog["correlations"]["anchors"],
        json!([{
            "aggregateType": "bike-rental/rental-fleet",
            "label": "Rental fleet",
            "correlationsHrefTemplate": format!("{FLEET}/{{aggregateId}}/correlations"),
        }])
    );

    command(
        &normal,
        "external-rent",
        "rent-bicycle",
        json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
    )
    .await?;
    let rent = indexed(&app, "external-rent", 2).await?;
    assert_eq!(rent["refs"][0]["kind"], "domain");
    assert_eq!(rent["refs"][0]["aggregate_id"], "city-fleet");
    assert_eq!(rent["refs"][0]["event_type"], "bicycle-rented");
    assert_eq!(rent["refs"][1]["kind"], "integration");
    assert_eq!(rent["refs"][1]["event_type"], "bicycle-rental-started");
    assert_eq!(rent["refs"][1]["causation_id"], rent["refs"][0]["event_id"]);
    assert_eq!(rent["refs"][1]["available"], true);

    command(
        &normal,
        "city-bike-7",
        "add-bicycle",
        json!({"fleet_id": "city-fleet", "bicycle_id": "bike-7", "condition": "serviceable"}),
    )
    .await?;
    command(
        &normal,
        "harbor-bike-8",
        "add-bicycle",
        json!({"fleet_id": "harbor-fleet", "bicycle_id": "bike-8", "condition": "serviceable"}),
    )
    .await?;
    command(
        &normal,
        "transfer-bike-7",
        "transfer-bicycle",
        json!({"from_fleet_id": "city-fleet", "bicycle_id": "bike-7", "to_fleet_id": "harbor-fleet"}),
    )
    .await?;
    indexed(&app, "transfer-bike-7", 2).await?;

    let (status, city) = get(
        &app,
        &format!("{FLEET}/city-fleet/correlations"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200);
    let city_correlations = correlation_ids(&city);
    assert_eq!(
        city_correlations.get(2..),
        Some(["external-rent", "city-bike-7", "transfer-bike-7"].as_slice())
    );
    assert_eq!(city["lag"], json!({"bike-rental": 0}));
    let rent = &city["correlations"][2]["refs"][0];
    assert_eq!(rent["payload"]["bicycle_id"], "bike-42");
    let (_, harbor) = get(
        &app,
        &format!("{FLEET}/harbor-fleet/correlations"),
        "inspection",
    )
    .await?;
    assert_eq!(
        correlation_ids(&harbor),
        ["harbor-bike-8", "transfer-bike-7"]
    );
    let transfer = harbor["correlations"][1]["refs"]
        .as_array()
        .ok_or("missing transfer refs")?;
    let source = transfer
        .iter()
        .find(|item| item["aggregate_id"] == "city-fleet")
        .ok_or("missing source fleet ref")?;
    assert!(source.get("payload").is_none());
    let (_, included) = get(
        &app,
        &format!("{FLEET}/harbor-fleet/correlations?include=context:bike-rental"),
        "inspection",
    )
    .await?;
    let source = included["correlations"][1]["refs"]
        .as_array()
        .and_then(|refs| {
            refs.iter()
                .find(|item| item["aggregate_id"] == "city-fleet")
        })
        .ok_or("missing included source fleet ref")?;
    assert_eq!(source["payload"]["bicycle_id"], "bike-7");

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
    assert_eq!(
        matched["matched"],
        json!([{"context": "bike-rental", "event_type": "bicycle-added",
            "field": "bicycle_id", "value": "bike-7"}])
    );

    let (status, unknown) =
        get(&app, &format!("{FLEET}/bike-42/correlations"), "inspection").await?;
    assert_eq!(status, 404, "a bicycle ID is not a fleet stream: {unknown}");
    assert_eq!(unknown["code"], "unknown-anchor-stream");

    for token in ["control", "dispatch"] {
        for path in [
            format!("{FLEET}/city-fleet/correlations"),
            "/correlations/external-rent/events".to_owned(),
        ] {
            assert_eq!(get(&app, &path, token).await?.0, 403, "{token} {path}");
        }
    }

    host.shutdown().await?;
    let reader = normal
        .correlation_index_reader()
        .await?
        .ok_or("missing correlation index")?;
    let chain = reader.chain(&CorrelationId::new("external-rent")?).await?;
    assert_eq!(chain.refs.len(), 2);
    let freshness = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let freshness = reader.freshness().await?;
            if freshness.lag.get("bike-rental") == Some(&None) {
                return TestResult::Ok(freshness);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    assert_eq!(freshness.sources.len(), 2);
    for source in &freshness.sources {
        assert_eq!(source.state, CorrelationSourceState::Inactive, "{source:?}");
        assert_eq!(source.behind, 0, "{source:?}");
    }
    connection.drain().await?;
    Ok(())
}
