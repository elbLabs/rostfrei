use std::{error::Error, io};

use bike_rental::{
    demo::{demo_stream, seed_demo},
    rental_fleet::{BicycleCondition, BicycleStatus, RentalFleetAggregate},
};
use rostfrei::core::OutcomeDecision;
use rostfrei::nats::{
    NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, connect, provision_event_store,
};
use rostfrei::{
    ApplicationName, CommandExecutor, CommandReceipt, OutcomeKey, OutcomeLookup, OutcomeReader,
};
use uuid::Uuid;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "one startup/restart scenario verifies native receipts and unchanged persisted state"
)]
async fn normal_demo_startup_persists_command_receipts_and_retries_without_writes() -> TestResult {
    let application = ApplicationName::new(format!("demo-seeding-{}", Uuid::now_v7()))?;
    let context = application.bounded_context("bike-rental")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_storage_limits(16 * 1024 * 1024, 512 * 1024)?
        .with_fresh_split_outcome_epoch("demo-seeding")?;
    let connection = connect(&NatsConnectionConfig::new(
        "demo-seeding",
        rostfrei::testing::integration::nats_url()?,
    ))
    .await?;

    let result: TestResult = async {
        provision_event_store(connection.jetstream(), &config).await?;
        let store = NatsEventStore::connect(connection.jetstream().clone(), config.clone()).await?;
        let first_receipts = seed_demo(&store).await?;
        ensure(
            first_receipts.len() == 2
                && first_receipts
                    .iter()
                    .all(|receipt| matches!(receipt, CommandReceipt::Appended(_))),
            "first startup must accept and append both AddBicycle commands",
        )?;
        let first = store.load(&demo_stream()).await?;
        ensure(
            first.len() == 2,
            "startup must add exactly the two demo bicycles",
        )?;
        let mut journal = connection
            .jetstream()
            .get_stream(config.stream_name())
            .await?;
        let state = &journal.info().await?.state;
        let first_watermark = (state.messages, state.last_sequence);

        // Reconnect to represent a later process startup with no in-memory seed state.
        let restarted =
            NatsEventStore::connect(connection.jetstream().clone(), config.clone()).await?;
        let retry_receipts = seed_demo(&restarted).await?;
        ensure(
            retry_receipts.len() == 2
                && retry_receipts
                    .iter()
                    .all(|receipt| matches!(receipt, CommandReceipt::ExactReplay(_))),
            "repeated startup must return both original command receipts as exact replays",
        )?;
        let retried = restarted.load(&demo_stream()).await?;
        ensure(
            retried == first,
            "repeated startup changed the committed demo events",
        )?;
        let state = &journal.info().await?.state;
        ensure(
            (state.messages, state.last_sequence) == first_watermark,
            "repeated startup added journal records instead of replaying its decisions",
        )?;

        let fleet = CommandExecutor::new(restarted.clone())
            .rehydrate::<RentalFleetAggregate>(&demo_stream())
            .await?;
        let bicycles: Vec<_> = fleet
            .state()
            .bicycles()
            .iter()
            .map(|bicycle| {
                (
                    bicycle.bicycle_id().as_str(),
                    bicycle.condition(),
                    bicycle.status(),
                )
            })
            .collect();
        ensure(
            bicycles
                == vec![
                    (
                        "bike-42",
                        BicycleCondition::Serviceable,
                        BicycleStatus::Available,
                    ),
                    (
                        "bike-99",
                        BicycleCondition::MaintenanceRequired,
                        BicycleStatus::Available,
                    ),
                ],
            "startup changed the named demo bicycles or their conditions",
        )?;

        for event in &retried {
            let key = OutcomeKey::new(
                restarted.authority().clone(),
                context.name().clone(),
                event.operation_id().clone(),
            );
            let lookup = restarted.lookup_outcome(&key).await?;
            let OutcomeLookup::Completed(outcome) = lookup else {
                return Err(io::Error::other(format!(
                    concat!(
                        "Normal demo startup must persist an accepted command receipt ",
                        "for each bicycle addition; operation {} has {:?}",
                    ),
                    event.operation_id().as_str(),
                    lookup,
                ))
                .into());
            };
            ensure(
                outcome.marker().decision() == OutcomeDecision::Accepted,
                "demo addition did not retain an accepted command decision",
            )?;
            ensure(
                outcome.marker().integrity_receipt().events() == vec![event.clone()],
                "each AddBicycle receipt must identify exactly its committed addition",
            )?;
            outcome.details().available()?;
        }
        ensure(
            retried
                .iter()
                .map(|event| event.operation_id().as_str())
                .eq([
                    "demo:add-bicycle:city-fleet:bike-42",
                    "demo:add-bicycle:city-fleet:bike-99",
                ]),
            "demo additions must use separate stable command operation identities",
        )
    }
    .await;

    let journal_cleanup = connection
        .delete_stream_if_exists(config.stream_name())
        .await;
    let state_cleanup = connection
        .delete_stream_if_exists(config.outcome_state_stream_name())
        .await;
    let drain = connection.drain().await;
    result?;
    journal_cleanup?;
    state_cleanup?;
    drain?;
    Ok(())
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
