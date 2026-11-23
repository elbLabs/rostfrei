//! A disposable Test-scoped example. Run it twice against the same local broker.
use std::{env, error::Error};

use async_trait::async_trait;
use rostfrei::{
    AggregateEvents, DomainEvent,
    core::{
        Aggregate, AggregateId, AggregateType, CommandDecision, CommandExecution,
        CommandExecutionMetadata, CommandExecutor, CommandHandler, CommandHandlingResult,
        CommandOutcome, CommandReceipt, ContentFingerprint, JsonRejectionCodec, OperationId,
        StreamId,
    },
    messaging::ApplicationName,
    nats::{
        NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, connect, provision_event_store,
    },
};
use serde::{Deserialize, Serialize};

rostfrei::install_macro_support!();

#[derive(Clone, Debug, Deserialize, DomainEvent, Serialize)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
struct BicycleRented {}

#[derive(AggregateEvents)]
enum BicycleEvent {
    BicycleRented(BicycleRented),
}

struct Bicycle;
impl Aggregate for Bicycle {
    type State = bool;
    type Event = BicycleEvent;
    const BOUNDED_CONTEXT: &'static str = "first-rental";
    const AGGREGATE_TYPE: &'static str = "bicycle";

    fn initial(_: &StreamId) -> bool {
        false
    }
    fn apply(rented: &mut bool, _: &BicycleEvent) {
        *rented = true;
    }
}

#[derive(Serialize)]
struct RentBicycle {
    bicycle_id: String,
}

struct RentHandler;

#[async_trait]
impl CommandHandler<RentBicycle> for RentHandler {
    type Rejection = String;

    async fn handle(
        &self,
        command: &RentBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<String> {
        let mut bicycle = execution.load::<Bicycle>(&command.bicycle_id).await?;
        if *bicycle.aggregate().state() {
            return Ok(CommandDecision::Rejected("already rented".to_owned()));
        }
        bicycle.aggregate_mut().raise(BicycleRented {});
        Ok(CommandDecision::Accepted)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = env::var("ROSTFREI_NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_owned());
    let connection = connect(&NatsConnectionConfig::new("starter-demo", url)).await?;
    let application = ApplicationName::new(env!("CARGO_PKG_NAME"))?;
    let context = application.test_bounded_context(Bicycle::BOUNDED_CONTEXT)?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?
        .with_fresh_outcomes()?
        .with_storage_limits(16 * 1024 * 1024, 64 * 1024)?;
    provision_event_store(connection.jetstream(), &config).await?;
    let store = NatsEventStore::connect(connection.jetstream().clone(), config).await?;
    let executor = CommandExecutor::new(store.clone());
    let command = RentBicycle {
        bicycle_id: "bike-1".to_owned(),
    };
    let metadata = CommandExecutionMetadata::new(
        OperationId::new("rental-1")?,
        ContentFingerprint::digest(serde_json::to_vec(&command)?),
    )
    .with_bounded_context(context.name().clone());
    let codec = JsonRejectionCodec::new("rental-rejection", 1);
    let first = executor
        .execute_with_rejection_codec(&RentHandler, metadata.clone(), &command, &codec)
        .await?;
    let retry = executor
        .execute_with_rejection_codec(&RentHandler, metadata, &command, &codec)
        .await?;
    let (
        CommandOutcome::Accepted(first),
        CommandOutcome::Accepted(CommandReceipt::ExactReplay(replayed)),
    ) = (first, retry)
    else {
        return Err("expected an accepted rental and its exact replay".into());
    };
    let stream = StreamId::new(
        AggregateType::new(Bicycle::AGGREGATE_TYPE)?,
        AggregateId::new(command.bicycle_id)?,
    );
    let history = store.load(&stream).await?;
    if history.len() != 1 || first.events() != history || replayed != history {
        return Err("rental receipt does not match the single recorded event".into());
    }
    let origin = if first.is_exact_replay() {
        "replayed"
    } else {
        "committed"
    };
    println!(
        "rental-1: {origin}; retry: replayed; stored events: {}",
        history.len()
    );
    connection.drain().await?;
    Ok(())
}
