#![allow(clippy::panic_in_result_fn)]

use std::{error::Error, sync::Arc};

use bike_rental::{
    BikeRentalNatsResourceLimits, FINANCE_CONTEXT, RENTAL_CONTEXT, WORKSHOP_CONTEXT,
    demo::{demo_fixture, demo_stream, returned_city_bike_fixture, seed_demo},
    tracer,
};
use rostfrei::messaging::TrafficScope;
use rostfrei::nats::{NatsConnection, NatsConnectionConfig, connect};
use rostfrei::tracer::{
    NatsApplication, NatsRuntime, NatsRuntimeConfig, TestScenarioReset, TracerConfig,
    http::HttpConfig,
};
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
        NatsRuntimeConfig::new(application.bounded_context(RENTAL_CONTEXT)?, &bindings, 1)?
            .with_fresh_outcomes()?;
    let normal =
        NatsRuntime::provision(connection.clone(), normal_config, bindings.clone()).await?;
    seed_demo(&normal.store()).await?;
    normal.start_workers().await?;
    let host = NatsApplication::builder(
        application.test_bounded_context(RENTAL_CONTEXT)?,
        connection.clone(),
        bindings,
    )?
    .with_fresh_outcomes()?
    .provision_and_start()
    .await?;
    let runtime = host
        .runtime_for(RENTAL_CONTEXT)
        .ok_or("missing Rental runtime")?;
    let stream = demo_stream();
    let normal_history = normal.store().load(&stream).await?;
    for _ in 0..2 {
        let old = runtime.store().clone();
        runtime.reset(&demo_fixture()?).await?;
        assert!(!runtime.store().load(&stream).await?.is_empty());
        let key = OutcomeKey::new(
            old.authority().clone(),
            application
                .test_bounded_context(RENTAL_CONTEXT)?
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

async fn stream_info(
    connection: &NatsConnection,
    runtime: &NatsRuntime,
) -> Result<[async_nats::jetstream::stream::Info; 2], Box<dyn Error + Send + Sync>> {
    let store = &runtime.config().event_store;
    let mut journal = connection
        .jetstream()
        .get_stream(store.stream_name())
        .await?;
    let mut outcomes = connection
        .jetstream()
        .get_stream(store.outcome_state_stream_name())
        .await?;
    Ok([
        journal.info().await?.clone(),
        outcomes.info().await?.clone(),
    ])
}

#[tokio::test]
async fn test_reset_recreates_each_members_authority_without_touching_normal() -> TestResult {
    let connection = connect(&NatsConnectionConfig::new(
        "grouped-reset",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let application = format!("grouped-reset-{}", uuid::Uuid::now_v7().simple());
    let limits = BikeRentalNatsResourceLimits::from_env()?;
    let normal = tracer::configurations(&application, TrafficScope::Normal, limits)?
        .with_fresh_outcomes()?
        .application_builder(connection.clone())?
        .provision_and_start()
        .await?;
    let rental = normal
        .runtime_for(RENTAL_CONTEXT)
        .ok_or("missing Normal Rental runtime")?;
    seed_demo(&rental.store()).await?;
    let history = rental.store().load(&demo_stream()).await?;
    let clean = demo_fixture()?;
    let test = tracer::configurations(&application, TrafficScope::Test, limits)?
        .with_fresh_outcomes()?
        .application_builder(connection.clone())?
        .with_default_test_fixture(returned_city_bike_fixture()?)
        .with_test_fixture(clean.clone())
        .with_tracer(Some(TracerConfig::new(
            tokio::net::TcpListener::bind("127.0.0.1:0").await?,
            HttpConfig::new("control")?,
        )))
        .provision_and_start()
        .await?;
    let payment = StreamId::new(
        rostfrei::StreamAggregateType::new("finance/rental-payment")?,
        rostfrei::StreamAggregateId::new("payment-42")?,
    );
    let finance = test
        .runtime_for(FINANCE_CONTEXT)
        .ok_or("missing Test Finance runtime")?;
    assert_eq!(finance.store().load(&payment).await?.len(), 3);

    let mut before = Vec::new();
    for context in [RENTAL_CONTEXT, FINANCE_CONTEXT, WORKSHOP_CONTEXT] {
        let normal = normal
            .runtime_for(context)
            .ok_or("missing Normal runtime")?;
        let test = test.runtime_for(context).ok_or("missing Test runtime")?;
        before.push((
            context,
            normal,
            stream_info(&connection, normal).await?,
            test,
            stream_info(&connection, test).await?,
        ));
    }
    test.tracer()
        .ok_or("missing Tracer")?
        .reset_test_scenario_with_fixture(clean.id())
        .await?;
    for (context, normal, normal_before, test, test_before) in before {
        for (old, current) in normal_before
            .iter()
            .zip(stream_info(&connection, normal).await?)
        {
            assert_eq!(
                current.created, old.created,
                "Normal {context}: {}",
                old.config.name
            );
            assert_eq!(
                current.state.last_sequence, old.state.last_sequence,
                "Normal {context}: {}",
                old.config.name
            );
        }
        for (old, current) in test_before
            .iter()
            .zip(stream_info(&connection, test).await?)
        {
            assert_ne!(
                current.created, old.created,
                "Test {context}: {}",
                old.config.name
            );
        }
    }
    assert!(finance.store().load(&payment).await?.is_empty());
    assert_eq!(rental.store().load(&demo_stream()).await?, history);
    test.shutdown().await?;
    normal.shutdown().await?;
    connection.drain().await?;
    Ok(())
}
