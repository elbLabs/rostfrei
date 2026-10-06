#![allow(clippy::panic_in_result_fn)]

use std::{error::Error, sync::Arc};

use bike_rental::{demo::demo_fixture, tracer};
use rostfrei::nats::{NatsConnectionConfig, connect};
use rostfrei::tracer::{NatsApplication, NatsRuntime, NatsRuntimeConfig, TestScenarioReset};
use rostfrei::{ApplicationName, OperationId, OutcomeKey, OutcomeReader, StreamId};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn paired_reset_reconnects_authority_preserves_normal_and_cannot_bypass_failure() -> TestResult
{
    let connection = connect(&NatsConnectionConfig::new(
        "split-host",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let application = ApplicationName::new(format!(
        "split-host-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ))?;
    let bindings = Arc::new(tracer::bindings()?);
    let normal_config =
        NatsRuntimeConfig::new(application.bounded_context("bike-rental")?, &bindings, 1)?
            .with_fresh_completed_outcome_epoch("owned-test")?;
    let normal =
        NatsRuntime::provision(connection.clone(), normal_config, bindings.clone()).await?;
    normal.apply_fixture(&demo_fixture()?).await?;
    normal.start_workers().await?;
    let host = NatsApplication::builder(
        application.test_bounded_context("bike-rental")?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_completed_outcome_epoch("owned-test")?
    .provision_and_start()
    .await?;
    let runtime = host.runtime();
    let stream = StreamId::new(
        rostfrei::StreamAggregateType::new("bike-rental/rental-fleet")?,
        rostfrei::StreamAggregateId::new("city-fleet")?,
    );
    let normal_history = normal.store().load(&stream).await?;
    for _ in 0..2 {
        let old = runtime.store().clone();
        runtime.reset(&demo_fixture()?).await?;
        assert!(!runtime.store().load(&stream).await?.is_empty());
        let key = OutcomeKey::new(
            old.authority().clone(),
            application
                .test_bounded_context("bike-rental")?
                .name()
                .clone(),
            OperationId::new("missing")?,
        );
        assert!(
            old.lookup_outcome(&key).await.is_err(),
            "old incarnation must remain fenced"
        );
        let current = runtime.store();
        let current_key = OutcomeKey::new(
            current.authority().clone(),
            key.bounded_context().clone(),
            key.operation_id().clone(),
        );
        assert!(
            current.lookup_outcome(&current_key).await.is_ok(),
            "new handle must admit its fresh pair"
        );
        assert_eq!(normal.store().load(&stream).await?, normal_history);
    }
    let mut invalid = serde_json::to_value(demo_fixture()?)?;
    invalid["messages"][0]["payload"] = serde_json::json!({"invalid":true});
    let invalid = serde_json::from_value(invalid)?;
    assert!(runtime.reset(&invalid).await.is_err());
    assert!(
        runtime.start_workers().await.is_err(),
        "start cannot reopen a failed reset"
    );
    runtime.reset(&demo_fixture()?).await?;
    assert_eq!(normal.store().load(&stream).await?, normal_history);
    host.shutdown().await?;
    normal.stop_workers().await;
    connection.drain().await?;
    Ok(())
}
