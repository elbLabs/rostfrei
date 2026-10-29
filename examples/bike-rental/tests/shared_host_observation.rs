#![allow(
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    reason = "end-to-end assertions retain the host lifecycle scenario"
)]

use std::{error::Error, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{body::Body, http::Request};
use bike_rental::{
    demo::{demo_fixture, demo_stream},
    tracer,
};
use http_body_util::BodyExt as _;
use rostfrei::nats::{NatsConnectionConfig, connect};
use rostfrei::tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandTransportObserver,
    NatsApplication, NatsRuntime, NatsRuntimeConfig, ObservationFeed, ObservationScope,
    ObservationStatus, OperationMode, TestScenarioReset, TracerConfig,
    command_execution_fingerprint,
    http::{self, HttpConfig},
};
use rostfrei::{ApplicationName, Command, OperationId};
use serde_json::{Value, json};
use tower::ServiceExt as _;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct Observer;
#[async_trait]
impl CommandTransportObserver for Observer {
    async fn command_published(&self, _: CommandPublication) {}
}

async fn rent(runtime: &NatsRuntime, correlation: &str) -> TestResult {
    let payload = json!({"fleet_id": "city-fleet", "bicycle_id": "bike-42"});
    let receipt = runtime
        .transport()
        .invoke(
            CommandInvocation::new(
                OperationId::new(correlation)?,
                correlation,
                command_execution_fingerprint(
                    "bike-rental",
                    "rent-bicycle",
                    bike_rental::rental_fleet::RentBicycle::SCHEMA_VERSION,
                    &payload,
                ),
                "bike-rental",
                "rent-bicycle",
                bike_rental::rental_fleet::RentBicycle::SCHEMA_VERSION,
                payload,
            ),
            Arc::new(Observer),
        )
        .await?;
    assert!(matches!(receipt.outcome(), CommandOutcome::Accepted));
    Ok(())
}

async fn flow(
    feed: &ObservationFeed,
    correlation: &str,
) -> TestResult<rostfrei::tracer::ObservedFlow> {
    Ok(tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(summary) = feed
                .snapshot()
                .items
                .iter()
                .find(|item| item.correlation_id == correlation && item.message_count == 2)
            {
                return feed.flow(&summary.id);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??)
}

async fn get(app: &axum::Router, path: &str, token: &str) -> TestResult<Value> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), 200);
    Ok(serde_json::from_slice(
        &response.into_body().collect().await?.to_bytes(),
    )?)
}

#[tokio::test]
async fn shared_host_observes_external_flows_and_owns_scope_readiness_across_reset_and_shutdown()
-> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "shared-observation",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("shared-observation-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let normal = Arc::new(
        NatsRuntime::provision(
            connection.clone(),
            NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
                .with_fresh_completed_outcome_epoch("observation")?,
            bindings.clone(),
        )
        .await?,
    );
    normal.apply_fixture(&demo_fixture()?).await?;
    let test_feed = ObservationFeed::new(application.clone(), ObservationScope::Test);
    let production_feed = ObservationFeed::new(application.clone(), ObservationScope::Production);
    let http = HttpConfig::new("control")?.with_inspection_token("inspection")?;
    let config = TracerConfig::new(
        tokio::net::TcpListener::bind("127.0.0.1:0").await?,
        http.clone(),
    )
    .with_dispatch_runtime(normal.clone())
    .with_continuous_observation(test_feed.clone())
    .with_continuous_observation(production_feed.clone());
    let host = NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("observation")?
    .with_default_test_fixture(demo_fixture()?)
    .with_tracer(Some(config))
    .provision_and_start()
    .await?;
    assert_eq!(
        test_feed.snapshot().status,
        ObservationStatus::Live,
        "Test ready before traffic"
    );
    assert_eq!(
        production_feed.snapshot().status,
        ObservationStatus::Live,
        "production ready before traffic"
    );
    let tracer = host.tracer().ok_or("missing Tracer")?.clone();
    let app = http::router(tracer.clone(), http);
    for (token, scope) in [("control", "test"), ("inspection", "production")] {
        let catalog = get(&app, "/catalog", token).await?;
        assert_eq!(catalog["catalogVersion"], 1);
        assert_eq!(
            catalog["observation"],
            json!([{
                "application": application.as_str(), "scope": scope,
                "listHref": format!("/observation/{scope}"),
                "eventsHref": format!("/observation/{scope}/events"),
            }])
        );
        let href = catalog["observation"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["listHref"].as_str())
            .ok_or("missing observation link")?;
        assert_eq!(get(&app, href, token).await?["status"], "live");
    }
    // Publish through application transports, without registering Tracer operations.
    rent(host.runtime(), "external-test").await?;
    rent(&normal, "external-production").await?;
    let test_flow = flow(&test_feed, "external-test").await?;
    let production_flow = flow(&production_feed, "external-production").await?;
    for (runtime, observed, token) in [
        (host.runtime(), &test_flow, "control"),
        (&normal, &production_flow, "inspection"),
    ] {
        assert!(
            tracer
                .correlation_mode(&observed.summary.correlation_id)
                .is_err()
        );
        let history = runtime.store().load(&demo_stream()).await?;
        let event = history.last().ok_or("missing persisted event")?;
        let series = &observed.message_series;
        assert!(series.messages().get(event.event_id().as_str()).is_some());
        assert!(series.messages().iter().any(|message| {
            message.name() == "bicycle-rental-started"
                && message.causation_id() == Some(event.event_id().as_str())
        }));
        assert!(
            series
                .messages()
                .iter()
                .all(|message| !message.is_command() && message.payload().is_none())
        );
        assert!(series.command_outcomes().is_empty());
        assert_eq!(
            get(&app, &observed.summary.detail_href, token).await?["partial"],
            true
        );
    }
    assert_eq!(test_feed.snapshot().items.len(), 1);
    assert_eq!(production_feed.snapshot().items.len(), 1);
    let production_generation = production_feed.snapshot().generation;
    tracer.reset_test_scenario().await?;
    assert_eq!(test_feed.snapshot().status, ObservationStatus::Live);
    assert!(test_feed.flow(&test_flow.summary.id).is_err());
    assert_ne!(test_feed.snapshot().generation, test_flow.generation);
    assert_eq!(production_feed.snapshot().generation, production_generation);
    assert!(production_feed.flow(&production_flow.summary.id).is_ok());
    assert_eq!(normal.store().load(&demo_stream()).await?.len(), 3);
    rent(host.runtime(), "external-after-reset").await?;
    flow(&test_feed, "external-after-reset").await?;
    host.shutdown().await?;
    assert_eq!(test_feed.snapshot().status, ObservationStatus::Unavailable);
    assert_eq!(
        production_feed.snapshot().status,
        ObservationStatus::Unavailable
    );
    // Source guards were released even though runtime, Tracer and feed handles survive.
    let replacement = test_feed.source("bike-rental")?;
    replacement.ready();
    assert_eq!(test_feed.snapshot().status, ObservationStatus::Live);
    drop(replacement);
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn runtime_observation_reports_interruption_and_failed_reset_then_recovers() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "observation-health",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("observation-health-{suffix}"))?;
    let bindings = Arc::new(tracer::bindings()?);
    let config = NatsRuntimeConfig::new(
        application.test_bounded_context("bike-rental")?,
        &bindings,
        1,
    )?
    .with_fresh_completed_outcome_epoch("observation")?;
    let runtime = Arc::new(
        NatsRuntime::provision(connection.clone(), config.clone(), bindings.clone()).await?,
    );
    let feed = ObservationFeed::new(application, ObservationScope::Test);
    let tracer = runtime
        .tracer_builder()
        .with_test_transport(runtime.transport())
        .with_continuous_observation(feed.clone())
        .with_default_test_fixture(demo_fixture()?)
        .with_test_scenario_reset(runtime.clone())
        .build()?;
    runtime
        .attach_observer(tracer.correlation_observer(OperationMode::Test))
        .await?;
    assert_eq!(feed.snapshot().status, ObservationStatus::Live);
    let mut updates = feed.subscribe()?;
    updates.next().await.ok_or("initial snapshot")?;
    // The subscription stays owned, but its broker coverage is interrupted.
    connection
        .delete_stream_if_exists(
            config
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if updates.next().await.ok_or("health update")?.status == ObservationStatus::Unavailable
            {
                return Ok::<_, Box<dyn Error + Send + Sync>>(());
            }
        }
    })
    .await??;
    config.provision(&connection).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if updates.next().await.ok_or("recovery update")?.status == ObservationStatus::Live {
                return Ok::<_, Box<dyn Error + Send + Sync>>(());
            }
        }
    })
    .await??;
    let mut invalid = serde_json::to_value(demo_fixture()?)?;
    drop(updates);
    for message in invalid["messages"]
        .as_array_mut()
        .ok_or("fixture messages")?
    {
        message["aggregate"]["type"] = json!("unknown/aggregate");
    }
    assert!(
        runtime
            .reset(&serde_json::from_value(invalid)?)
            .await
            .is_err()
    );
    assert_eq!(feed.snapshot().status, ObservationStatus::Unavailable);
    assert!(runtime.start_workers().await.is_err());
    tracer.reset_test_scenario().await?;
    assert_eq!(feed.snapshot().status, ObservationStatus::Live);
    runtime.stop_workers().await;
    assert_eq!(feed.snapshot().status, ObservationStatus::Unavailable);
    // A subscription startup error must release its newly registered source too.
    connection
        .delete_stream_if_exists(
            config
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?;
    assert!(
        runtime
            .start_correlation_observer(tracer.correlation_observer(OperationMode::Test))
            .await
            .is_err()
    );
    assert_eq!(feed.snapshot().status, ObservationStatus::Unavailable);
    config.provision(&connection).await?;
    runtime.start_workers().await?;
    assert_eq!(feed.snapshot().status, ObservationStatus::Live);
    runtime.stop_workers().await;
    connection.drain().await?;
    Ok(())
}

#[tokio::test]
async fn host_rejects_observation_without_a_matching_application_runtime() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "observation-config",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let application = ApplicationName::new(format!("observation-config-{suffix}"))?;
    let context = application.test_bounded_context("bike-rental")?;
    let bindings = Arc::new(tracer::bindings()?);
    let resources = NatsRuntimeConfig::new(context.clone(), &bindings, 1)?;
    for feed in [
        ObservationFeed::new(
            ApplicationName::new("wrong-application")?,
            ObservationScope::Test,
        ),
        ObservationFeed::new(application, ObservationScope::Production),
    ] {
        let config = TracerConfig::new(
            tokio::net::TcpListener::bind("127.0.0.1:0").await?,
            HttpConfig::new("control")?,
        )
        .with_continuous_observation(feed);
        let result =
            NatsApplication::builder(context.clone(), connection.clone(), bindings.clone())?
                .with_default_test_fixture(demo_fixture()?)
                .with_tracer(Some(config))
                .provision_and_start()
                .await;
        assert!(matches!(
            result,
            Err(rostfrei::tracer::NatsHostError::Configuration(_))
        ));
        assert!(
            connection
                .jetstream()
                .get_stream(resources.event_store.stream_name())
                .await
                .is_err(),
            "invalid observation configuration must fail before provisioning"
        );
    }
    connection.drain().await?;
    Ok(())
}
