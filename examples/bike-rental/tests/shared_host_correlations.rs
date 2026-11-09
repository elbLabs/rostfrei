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
use rostfrei::nats::{NatsConnection, NatsConnectionConfig, NatsEventStoreConfig, connect};
use rostfrei::tracer::{
    ApplicationBindings, CommandInvocation, CommandOutcome, CommandPublication,
    CommandTransportObserver, NatsApplication, NatsHostError, NatsRuntime, NatsRuntimeConfig,
    TracerConfig, command_execution_fingerprint,
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

async fn start_inspection_host(
    application: &ApplicationName,
    connection: &NatsConnection,
    bindings: Arc<ApplicationBindings>,
    config: TracerConfig,
) -> TestResult<NatsApplication> {
    Ok(NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("retained-correlations")?
    .with_default_test_fixture(demo_fixture()?)
    .with_tracer(Some(config))
    .provision_and_start()
    .await?)
}

fn assert_source_cutoffs(body: &Value, normal: &NatsRuntime) -> TestResult {
    let sources = body
        .pointer("/snapshot/sources")
        .and_then(Value::as_array)
        .ok_or("missing source cutoffs")?;
    assert_eq!(
        sources.len(),
        2,
        "domain source must be registered exactly once"
    );
    let domain = sources
        .iter()
        .find(|source| source["kind"] == "domain")
        .ok_or("missing domain cutoff")?;
    assert_eq!(domain["context"], "bike-rental");
    assert_eq!(domain["stream"], normal.config().event_store.stream_name());
    assert!(sources.iter().any(|source| source["kind"] == "integration"));
    for source in sources {
        let cutoff = source["cutoff_sequence"]
            .as_str()
            .ok_or("cutoff must retain its decimal string representation")?;
        assert!(cutoff.parse::<u64>()? > 0);
    }
    Ok(())
}

#[tokio::test]
async fn dispatch_runtime_automatically_registers_retained_inspection_sources() -> TestResult {
    inspect_runtime_source(false).await
}

#[tokio::test]
async fn inspection_only_host_deduplicates_explicit_sources_without_dispatch() -> TestResult {
    inspect_runtime_source(true).await
}

#[tokio::test]
async fn normal_hosts_reject_fixture_injection_before_writing() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "normal-fixture-rejection",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("normal-fixture-rejection-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let config = NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
        .with_fresh_completed_outcome_epoch("normal-fixture-rejection")?;
    let runtime =
        NatsRuntime::provision(connection.clone(), config.clone(), bindings.clone()).await?;
    let rejected = runtime.apply_fixture(&demo_fixture()?).await;
    assert!(
        matches!(rejected, Err(NatsHostError::Configuration(message)) if message.contains("Test scope"))
    );
    assert!(
        runtime
            .store()
            .load(&bike_rental::demo::demo_stream())
            .await?
            .is_empty()
    );
    let rejected = NatsApplication::builder(
        application.bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_runtime_config(config)
    .with_initial_fixture(demo_fixture()?)
    .provision_and_start()
    .await;
    assert!(
        matches!(rejected, Err(NatsHostError::Configuration(message)) if message.contains("Test runtime"))
    );
    assert!(
        runtime
            .store()
            .load(&bike_rental::demo::demo_stream())
            .await?
            .is_empty()
    );
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn inspection_host_survives_missing_outcome_state_but_rejects_replaced_journal() -> TestResult
{
    let connection = connect(&NatsConnectionConfig::new(
        "journal-history-host",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("journal-history-host-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let runtime = NatsRuntime::provision(
        connection.clone(),
        NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
            .with_fresh_completed_outcome_epoch("journal-history-host")?,
        bindings.clone(),
    )
    .await?;
    runtime.start_workers().await?;
    command(
        &runtime,
        "retained-add",
        "add-bicycle",
        json!({
            "fleet_id": "city-fleet", "bicycle_id": "bike-42", "condition": "serviceable"
        }),
    )
    .await?;
    command(
        &runtime,
        "retained-rent",
        "rent-bicycle",
        json!({
            "fleet_id": "city-fleet", "bicycle_id": "bike-42"
        }),
    )
    .await?;
    let config = runtime.config().event_store.clone();
    let mut original = None;
    for outcome_state_missing in [false, true] {
        if outcome_state_missing {
            connection
                .delete_stream_if_exists(config.outcome_state_stream_name())
                .await?;
            assert!(
                rostfrei::nats::NatsEventStore::connect(
                    connection.jetstream().clone(),
                    config.clone()
                )
                .await
                .is_err(),
                "mutable attachment must still require the live authority pair"
            );
        }
        let http = HttpConfig::inspection_only("inspection")?;
        let tracer_config = TracerConfig::new(
            tokio::net::TcpListener::bind("127.0.0.1:0").await?,
            http.clone(),
        )
        .with_production_event_store(config.clone())
        .with_trace_payload_policy(Arc::new(
            rostfrei::tracer::ExposeTracePayloadsForLocalDevelopment,
        ));
        let host =
            start_inspection_host(&application, &connection, bindings.clone(), tracer_config)
                .await?;
        let app = http::router(host.tracer().ok_or("missing Tracer")?.clone(), http);
        published_chain(&app, "retained-rent", 2).await?;
        runtime.stop_workers().await;
        let path = format!("{FLEET}/city-fleet/correlations?correlation=retained-rent");
        let (status, retained) = get(&app, &path, "inspection").await?;
        assert_eq!(status, 200, "{retained}");
        assert_eq!(correlation_ids(&retained), ["retained-rent"]);
        if let Some(original) = &original {
            assert_eq!(
                &retained, original,
                "journal evidence survives loss of live result state"
            );
            connection
                .delete_stream_if_exists(config.stream_name())
                .await?;
            rostfrei::nats::provision_event_store(connection.jetstream(), &config).await?;
            let (status, replaced) = get(&app, &path, "inspection").await?;
            assert_eq!(
                status, 503,
                "a replaced source must not look like an empty anchor: {replaced}"
            );
            assert!(matches!(
                replaced["code"].as_str(),
                Some("history-read-failed" | "correlation-read-failed")
            ));
        } else {
            original = Some(retained);
        }
        host.shutdown().await?;
    }
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn aggregate_correlation_selection_intersects_history_and_preserves_retained_evidence()
-> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "selected-correlations-host",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("selected-correlations-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let normal = NatsRuntime::provision(
        connection.clone(),
        NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
            .with_fresh_completed_outcome_epoch("selected-correlations")?,
        bindings.clone(),
    )
    .await?;
    normal.start_workers().await?;
    let http = HttpConfig::inspection_only("inspection")?;
    let config = TracerConfig::new(
        tokio::net::TcpListener::bind("127.0.0.1:0").await?,
        http.clone(),
    )
    .with_production_event_store(normal.config().event_store.clone())
    .with_trace_payload_policy(Arc::new(
        rostfrei::tracer::ExposeTracePayloadsForLocalDevelopment,
    ));
    let host = start_inspection_host(&application, &connection, bindings, config).await?;
    let app = http::router(host.tracer().ok_or("missing Tracer")?.clone(), http);

    // Commands establish both fleets and unrequested city history without fixtures.
    for (operation, fleet, bicycle) in [
        ("city-bike-42", "city-fleet", "bike-42"),
        ("city-bike-7", "city-fleet", "bike-7"),
        ("harbor-bike-8", "harbor-fleet", "bike-8"),
    ] {
        command(
            &normal,
            operation,
            "add-bicycle",
            json!({"fleet_id": fleet, "bicycle_id": bicycle, "condition": "serviceable"}),
        )
        .await?;
    }
    command(
        &normal,
        "selected-rent",
        "rent-bicycle",
        json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"}),
    )
    .await?;
    command(
        &normal,
        "selected-transfer",
        "transfer-bicycle",
        json!({
            "from_fleet_id": "city-fleet", "bicycle_id": "bike-7", "to_fleet_id": "harbor-fleet"
        }),
    )
    .await?;
    let rent = published_chain(&app, "selected-rent", 2).await?;
    published_chain(&app, "selected-transfer", 2).await?;
    // Freeze writes so every subsequent read must preserve the exact same cutoffs.
    normal.stop_workers().await;
    host.shutdown().await?;

    let (status, original) = get(
        &app,
        &format!("{FLEET}/city-fleet/correlations?include=context:bike-rental"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "{original}");
    assert_eq!(
        correlation_ids(&original),
        [
            "city-bike-42",
            "city-bike-7",
            "selected-rent",
            "selected-transfer"
        ]
    );
    assert_source_cutoffs(&original, &normal)?;
    let rent_refs = rent["refs"].as_array().ok_or("missing rental refs")?;
    let rented = rent_refs
        .iter()
        .find(|reference| reference["kind"] == "domain")
        .ok_or("missing rental domain event")?;
    let published = rent_refs
        .iter()
        .find(|reference| reference["kind"] == "integration")
        .ok_or("missing rental integration event")?;
    assert_eq!(published["causation_id"], rented["event_id"]);

    // Repeated IDs select actual city event correlations, not a union with harbor history.
    let selection = "correlation=selected-rent&correlation=selected-transfer&correlation=harbor-bike-8&correlation=not-recorded";
    let path = format!("{FLEET}/city-fleet/correlations?{selection}");
    let (status, selected) = get(
        &app,
        &format!("{path}&include=context:bike-rental"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "explicit correlation selection: {selected}");
    assert_eq!(
        correlation_ids(&selected),
        ["selected-rent", "selected-transfer"]
    );
    assert_eq!(selected["anchor"], original["anchor"]);
    assert_eq!(selected["snapshot"], original["snapshot"]);
    assert!(selected.get("truncated").is_none());
    assert_eq!(
        selected["correlations"],
        json!([original["correlations"][2], original["correlations"][3]]),
        "selection must preserve original references, causal identities, and payloads"
    );
    let transfer_refs = selected["correlations"][1]["refs"]
        .as_array()
        .ok_or("missing selected transfer refs")?;
    let destination = transfer_refs
        .iter()
        .find(|reference| reference["aggregate_id"] == "harbor-fleet")
        .ok_or("missing included destination event")?;
    assert_eq!(destination["payload"]["bicycle_id"], "bike-7");

    let (status, harbor) = get(
        &app,
        &format!("{FLEET}/harbor-fleet/correlations?include=context:bike-rental"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "{harbor}");
    assert_eq!(
        correlation_ids(&harbor),
        ["harbor-bike-8", "selected-transfer"]
    );
    assert_eq!(harbor["snapshot"], original["snapshot"]);
    assert_eq!(harbor["correlations"][1], selected["correlations"][1]);
    let source = harbor["correlations"][1]["refs"]
        .as_array()
        .ok_or("missing destination-anchored transfer")?
        .iter()
        .find(|reference| reference["aggregate_id"] == "city-fleet")
        .ok_or("missing included source event")?;
    assert_eq!(source["payload"]["bicycle_id"], "bike-7");

    let payload_match = "match=bike-rental/bicycle-transferred-in.bicycle_id=bike-7";
    let (status, matched) = get(
        &app,
        &format!("{path}&include=context:bike-rental&{payload_match}"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "{matched}");
    assert_eq!(correlation_ids(&matched), ["selected-transfer"]);
    assert_eq!(matched["snapshot"], original["snapshot"]);
    assert_eq!(matched["correlations"][0], selected["correlations"][1]);

    let (status, empty) = get(
        &app,
        &format!(
            "{FLEET}/city-fleet/correlations?correlation=harbor-bike-8&correlation=not-recorded"
        ),
        "inspection",
    )
    .await?;
    assert_eq!(status, 200, "{empty}");
    assert_eq!(empty["correlations"], json!([]));
    assert!(empty.get("truncated").is_none());
    let (status, unknown) = get(
        &app,
        &format!("{FLEET}/missing-fleet/correlations?{selection}"),
        "inspection",
    )
    .await?;
    assert_eq!(status, 404, "{unknown}");
    assert_eq!(unknown["code"], "unknown-anchor-stream");
    connection.drain().await?;
    Ok(())
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

async fn inspect_runtime_source(inspection_only: bool) -> TestResult {
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
    );
    let config = if inspection_only {
        normal.start_workers().await?;
        config
            .with_production_event_store(normal.config().event_store.clone())
            .with_production_event_store(normal.config().event_store.clone())
    } else {
        config.with_dispatch_runtime(normal.clone())
    };
    let host = start_inspection_host(&application, &connection, bindings, config).await?;
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
        "seed-city-bike-42",
        "add-bicycle",
        json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42", "condition": "serviceable"}),
    )
    .await?;
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
    let event_id = domain["event_id"]
        .as_str()
        .ok_or("missing domain event identity")?;
    assert!(!event_id.is_empty());
    assert_eq!(integration["causation_id"], event_id);
    assert!(
        integration["subject"]
            .as_str()
            .is_some_and(|subject| subject.contains(".external-rent."))
    );
    assert!(rent.get("lag").is_none());
    assert_source_cutoffs(&rent, &normal)?;
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
    assert_eq!(
        after_shutdown, rent,
        "shutdown preserves evidence and source cutoffs"
    );
    connection.drain().await?;
    Ok(())
}
