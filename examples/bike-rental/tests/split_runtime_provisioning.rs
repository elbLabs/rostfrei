#![allow(
    clippy::panic_in_result_fn,
    reason = "integration assertions report behavior failures"
)]

use std::{error::Error, sync::Arc, time::Duration};

use async_trait::async_trait;
use bike_rental::{
    demo::{demo_fixture, demo_stream},
    rental_fleet::RentBicycle,
    tracer,
};
use rostfrei::nats::{NatsConnectionConfig, connect};
use rostfrei::tracer::{
    CommandInvocation, CommandOutcome, CommandPublication, CommandTransportObserver,
    NatsApplication, NatsRuntimeConfig, command_execution_fingerprint,
};
use rostfrei::{ApplicationName, Command, OperationId};
use serde_json::json;

struct Observer;
#[async_trait]
impl CommandTransportObserver for Observer {
    async fn command_published(&self, _: CommandPublication) {}
}

#[tokio::test]
async fn independent_provisioning_prepares_the_complete_mapping_pipeline()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let connection = connect(&NatsConnectionConfig::new(
        "split-provisioning",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let context = ApplicationName::new(format!("split-provisioning-{suffix}"))?
        .test_bounded_context("bike-rental")?;
    let bindings = Arc::new(tracer::bindings()?);
    let config = NatsRuntimeConfig::new(context.clone(), &bindings, 1)?
        .with_fresh_completed_outcome_epoch("split-provisioning")?;
    config.provision(&connection).await?;
    config.provision(&connection).await?;
    let host = NatsApplication::builder(context.clone(), connection.clone(), bindings.clone())?
        .with_runtime_config(config.clone())
        .with_initial_fixture(demo_fixture()?)
        .connect_and_start()
        .await?;
    let payload = json!({"fleet_id":"city-fleet", "bicycle_id":"bike-42"});
    let receipt = host
        .runtime()
        .transport()
        .invoke(
            CommandInvocation::new(
                OperationId::new("split-rent")?,
                "split-rent",
                command_execution_fingerprint(
                    "bike-rental",
                    RentBicycle::LOCAL_ID,
                    RentBicycle::SCHEMA_VERSION,
                    &payload,
                ),
                "bike-rental",
                RentBicycle::LOCAL_ID,
                RentBicycle::SCHEMA_VERSION,
                payload,
            ),
            Arc::new(Observer),
        )
        .await?;
    assert!(matches!(receipt.outcome(), CommandOutcome::Accepted));
    let mut stream = connection
        .jetstream()
        .get_stream(
            config
                .messaging
                .topology()
                .integration_event_stream()
                .as_str(),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if stream.info().await?.state.messages == 1 {
                return Ok::<_, Box<dyn Error + Send + Sync>>(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    let history = host.runtime().store().load(&demo_stream()).await?;
    let rented = history.last().ok_or("missing persisted rental")?;
    let event = stream
        .get_raw_message(stream.cached_info().state.last_sequence)
        .await?;
    let envelope: rostfrei::IntegrationEventEnvelope<bike_rental::BicycleRentalStarted> =
        serde_json::from_slice(&event.payload)?;
    assert_eq!(
        envelope.causation_id().map(rostfrei::CausationId::as_str),
        Some(rented.event_id().as_str())
    );
    assert_eq!(envelope.correlation_id().as_str(), "split-rent");
    host.shutdown().await?;
    // Connect is verification-only: losing a required durable is surfaced, not repaired.
    let events = connection
        .jetstream()
        .get_stream(config.event_store.stream_name())
        .await?;
    events
        .delete_consumer(config.domain_events.durable_name().as_str())
        .await?;
    assert!(
        NatsApplication::builder(context, connection.clone(), bindings)?
            .with_runtime_config(config.clone())
            .connect_and_start()
            .await
            .is_err()
    );
    assert!(
        events
            .get_consumer::<async_nats::jetstream::consumer::pull::Config>(
                config.domain_events.durable_name().as_str()
            )
            .await
            .is_err()
    );
    connection.drain().await?;
    Ok(())
}
