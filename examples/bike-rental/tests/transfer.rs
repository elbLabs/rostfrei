use std::{env, error::Error, io, sync::Arc, time::Duration};

use bike_rental::{
    BikeRentalCommand, BikeRentalNatsConfig, BikeRentalNatsResourceLimits, BikeRentalNatsRuntime,
    demo::{apply_demo_fixture, demo_fixture, demo_stream},
    rental_fleet::{
        self, BicycleCondition, BicycleId, BicycleStatus, BicycleTransferRejectionReason,
        BicycleTransferredIn, BicycleTransferredOut, FleetId, RentalFleetAggregate,
        TransferBicycle, TransferBicycleHandler,
    },
};
use rostfrei::{
    CommandBus, CommandDecision, CommandExecutionMetadata, CommandExecutor, CommandMessageAdapter,
    CommandOutcome, CommandReceipt, CommandRequest, ContentFingerprint, DomainEvent, EventStore,
    OperationId,
};
use rostfrei_messaging_core::{CausationId, CommandResponseOutcome, CorrelationId};
use rostfrei_nats::{
    NatsConnection, NatsConnectionConfig, NatsEventStore, NatsEventStoreConfig, ServerVersion,
    connect, provision_event_store,
};
use uuid::Uuid;

const DESTINATION_FLEET_ID: &str = "harbor-fleet";

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn executor_atomically_transfers_replays_and_rejects_in_memory() -> TestResult {
    let store = rostfrei::InMemoryEventStore::new();
    apply_demo_fixture(&store).await?;

    verify_executor_transfer(store.clone()).await?;

    let rejected = CommandExecutor::new(store.clone())
        .execute(
            &TransferBicycleHandler,
            metadata("reject-missing-transfer")?,
            &transfer_command("missing-bike")?,
        )
        .await?;
    let CommandOutcome::Rejected(rejection) = rejected else {
        return Err(io::Error::other("missing bicycle transfer was not rejected").into());
    };
    ensure(
        rejection.reason == BicycleTransferRejectionReason::SourceBicycleMissing,
        "transfer was rejected for the wrong reason",
    )?;
    ensure(
        store.load(&demo_stream()).await?.len() == 2,
        "rejected transfer appended to the source stream",
    )?;
    ensure(
        store.load(&destination_stream()?).await?.len() == 1,
        "rejected transfer appended to the destination stream",
    )?;

    let same_fleet = CommandExecutor::new(store.clone())
        .execute(
            &TransferBicycleHandler,
            metadata("reject-same-fleet-transfer")?,
            &same_fleet_transfer_command("bike-99")?,
        )
        .await?;
    let CommandOutcome::Rejected(rejection) = same_fleet else {
        return Err(io::Error::other("same-fleet transfer was not rejected").into());
    };
    ensure(
        rejection.reason == BicycleTransferRejectionReason::SameFleet,
        "same-fleet transfer did not return its domain rejection",
    )?;
    ensure(
        store.load(&demo_stream()).await?.len() == 2
            && store.load(&destination_stream()?).await?.len() == 1,
        "same-fleet rejection appended an event",
    )
}

#[tokio::test]
async fn simulation_reports_both_participants_without_appending() -> TestResult {
    let store = rostfrei::InMemoryEventStore::new();
    apply_demo_fixture(&store).await?;

    let outcome = CommandExecutor::new(store.clone())
        .simulate(
            &TransferBicycleHandler,
            metadata("simulate-transfer")?,
            &transfer_command("bike-42")?,
        )
        .await?;

    ensure(
        matches!(outcome.decision(), CommandDecision::Accepted),
        "simulation did not accept with one primary event",
    )?;
    let [source, destination] = outcome.participants() else {
        return Err(io::Error::other("simulation did not expose exactly two participants").into());
    };
    ensure(
        source.stream_id() == &demo_stream()
            && source.events().len() == 1
            && source
                .events()
                .first()
                .is_some_and(|event| event.event_type() == BicycleTransferredOut::LOCAL_ID),
        "first simulation participant is not the source transfer event",
    )?;
    ensure(
        destination.stream_id() == &destination_stream()?
            && destination.events().len() == 1
            && destination
                .events()
                .first()
                .is_some_and(|event| event.event_type() == BicycleTransferredIn::LOCAL_ID),
        "simulation additional participant is not the destination transfer event",
    )?;
    ensure(
        store.load(&demo_stream()).await?.len() == 1
            && store.load(&destination_stream()?).await?.is_empty(),
        "simulation changed persisted history",
    )
}

#[tokio::test]
async fn rejected_simulation_reports_the_loaded_destination_as_a_read_participant() -> TestResult {
    let store = rostfrei::InMemoryEventStore::new();
    apply_demo_fixture(&store).await?;

    let outcome = CommandExecutor::new(store)
        .simulate(
            &TransferBicycleHandler,
            metadata("simulate-rejected-transfer")?,
            &transfer_command("missing-bike")?,
        )
        .await?;

    ensure(
        matches!(outcome.decision(), CommandDecision::Rejected(rejection) if rejection.reason == BicycleTransferRejectionReason::SourceBicycleMissing),
        "simulation did not preserve the transfer rejection",
    )?;
    let [source, destination] = outcome.participants() else {
        return Err(io::Error::other(
            "rejected simulation did not expose both loaded participants",
        )
        .into());
    };
    ensure(
        source.stream_id() == &demo_stream() && source.events().is_empty(),
        "rejected simulation changed the source participant",
    )?;
    ensure(
        destination.stream_id() == &destination_stream()?
            && destination.events().is_empty()
            && destination.is_read_guard(),
        "rejected simulation did not expose the destination read participant",
    )
}

#[tokio::test]
#[ignore = "requires NATS 2.12.1+"]
async fn executor_atomically_transfers_with_actual_nats_event_store() -> TestResult {
    let nats_url = required_nats_url()?;
    let unique = Uuid::now_v7();
    let application = rostfrei::ApplicationName::new(format!("transfer-executor-{unique}"))?;
    let context = application.test_bounded_context("bike-rental")?;
    let config = NatsEventStoreConfig::for_bounded_context(&context)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("transfer-executor-{unique}"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let result: TestResult = async {
        provision_event_store(connection.jetstream(), &config).await?;
        let store = NatsEventStore::connect(connection.jetstream().clone(), config.clone()).await?;
        apply_demo_fixture(&store).await?;
        verify_executor_transfer(store).await
    }
    .await;
    let cleanup = connection
        .delete_stream_if_exists(config.stream_name())
        .await;
    let drain = connection.drain().await;

    result?;
    cleanup?;
    drain?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS 2.12.1+"]
async fn command_bus_to_nats_worker_transfers_bicycle_across_two_streams() -> TestResult {
    let nats_url = required_nats_url()?;
    let unique = Uuid::now_v7();
    let application = format!("transfer-transport-{unique}");
    let limits = BikeRentalNatsResourceLimits::from_env()?;
    let config = BikeRentalNatsConfig::new_with_resource_limits(&application, limits)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("transfer-transport-{unique}"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;
    let result: TestResult = async {
        let runtime = Arc::new(
            BikeRentalNatsRuntime::provision_with_resource_limits(
                connection.clone(),
                &application,
                limits,
            )
            .await?,
        );
        let test_result = async {
            runtime.apply_fixture(&demo_fixture()?).await?;
            runtime.start_workers().await?;
            dispatch_transfer_through_nats(&connection, &runtime).await
        }
        .await;
        runtime.stop_workers().await;
        test_result
    }
    .await;
    let cleanup = cleanup_runtime_resources(&connection, &config).await;
    let drain = connection.drain().await;

    result?;
    cleanup?;
    drain?;
    Ok(())
}

async fn dispatch_transfer_through_nats(
    connection: &NatsConnection,
    runtime: &BikeRentalNatsRuntime,
) -> TestResult {
    let topology = runtime.config().messaging().topology().clone();
    let messaging = Arc::new(
        connection
            .messaging_adapter(topology)
            .with_response_timeout(Duration::from_secs(30)),
    );
    let command_adapter: Arc<dyn CommandMessageAdapter> = messaging;
    let command_bus = CommandBus::new(runtime.config().context().clone(), command_adapter);
    let correlation = CorrelationId::new("transfer-command-transport-correlation")?;
    let causation = CausationId::new("transfer-command-transport-causation")?;

    let receipt = command_bus
        .dispatch::<TransferBicycle>(
            CommandRequest::new(
                OperationId::new("transfer-command-transport-operation")?,
                transfer_command("bike-42")?,
            )
            .with_correlation_id(correlation.clone())
            .with_causation_id(causation.clone()),
        )
        .await?;

    ensure(
        matches!(
            receipt.response().outcome(),
            CommandResponseOutcome::Accepted
        ),
        "NATS command worker did not accept the transfer",
    )?;
    ensure(
        runtime
            .config()
            .command_route(BikeRentalCommand::TransferBicycle)
            .address()
            .as_str()
            .ends_with(".transfer-bicycle"),
        "transfer was not dispatched on the configured transfer command route",
    )?;

    let source = runtime.store().load(&demo_stream()).await?;
    let destination = runtime.store().load(&destination_stream()?).await?;
    ensure(
        source.len() == 2 && destination.len() == 1,
        "transported transfer did not append exactly once to both streams",
    )?;
    let outgoing = source
        .last()
        .ok_or_else(|| io::Error::other("source transfer event is absent"))?;
    let incoming = destination
        .first()
        .ok_or_else(|| io::Error::other("destination transfer event is absent"))?;
    ensure(
        outgoing.event_type() == BicycleTransferredOut::LOCAL_ID
            && incoming.event_type() == BicycleTransferredIn::LOCAL_ID,
        "transported transfer persisted the wrong event pair",
    )?;
    ensure(
        outgoing.operation_id().as_str() == "transfer-command-transport-operation"
            && incoming.operation_id() == outgoing.operation_id()
            && outgoing.correlation_id() == Some(&correlation)
            && incoming.correlation_id() == Some(&correlation)
            && outgoing.causation_id() == Some(&causation)
            && incoming.causation_id() == Some(&causation),
        "transported transfer did not preserve one operation and control context across streams",
    )
}

async fn verify_executor_transfer<S>(store: S) -> TestResult
where
    S: EventStore + Clone,
{
    let command = transfer_command("bike-42")?;
    let metadata = metadata("transfer-bike-42")?;
    let executor = CommandExecutor::new(store.clone());

    let first = executor
        .execute(&TransferBicycleHandler, metadata.clone(), &command)
        .await?;
    let CommandOutcome::Accepted(CommandReceipt::Appended(events)) = first else {
        return Err(io::Error::other("first transfer was not appended").into());
    };
    verify_event_pair(&events)?;

    let replay = executor
        .execute(&TransferBicycleHandler, metadata, &command)
        .await?;
    let CommandOutcome::Accepted(CommandReceipt::ExactReplay(replayed)) = replay else {
        return Err(io::Error::other("identical transfer was not an exact replay").into());
    };
    ensure(replayed == events, "exact replay returned different events")?;

    let source = executor
        .rehydrate::<RentalFleetAggregate>(&demo_stream())
        .await?;
    let destination = executor
        .rehydrate::<RentalFleetAggregate>(&destination_stream()?)
        .await?;
    ensure(
        source.state().bicycles().len() == 1
            && source
                .state()
                .bicycles()
                .first()
                .is_some_and(|bicycle| bicycle.bicycle_id().as_str() == "bike-99"),
        "source replay did not remove only the transferred bicycle",
    )?;
    let bicycle = destination
        .state()
        .bicycles()
        .first()
        .ok_or_else(|| io::Error::other("destination replay has no bicycle"))?;
    ensure(
        destination.state().bicycles().len() == 1
            && bicycle.bicycle_id().as_str() == "bike-42"
            && bicycle.status() == BicycleStatus::Available
            && bicycle.condition() == BicycleCondition::Serviceable,
        "destination replay did not restore the transferred bicycle",
    )
}

fn verify_event_pair(events: &[rostfrei::RecordedEvent]) -> TestResult {
    let [outgoing, incoming] = events else {
        return Err(io::Error::other("transfer receipt did not contain two events").into());
    };
    ensure(
        outgoing.stream_id() == &demo_stream()
            && outgoing.event_type() == BicycleTransferredOut::LOCAL_ID,
        "first transfer receipt event is not the source event",
    )?;
    ensure(
        incoming.stream_id() == &destination_stream()?
            && incoming.event_type() == BicycleTransferredIn::LOCAL_ID,
        "second transfer receipt event is not the destination event",
    )
}

fn metadata(operation: &str) -> TestResult<CommandExecutionMetadata> {
    Ok(CommandExecutionMetadata::new(
        OperationId::new(operation)?,
        ContentFingerprint::digest(operation),
    )
    .with_bounded_context(rostfrei::BoundedContextName::new("bike-rental")?))
}

fn transfer_command(bicycle: &str) -> TestResult<TransferBicycle> {
    Ok(TransferBicycle {
        from_fleet_id: FleetId::new("city-fleet")
            .ok_or_else(|| io::Error::other("invalid source fleet identity"))?,
        bicycle_id: BicycleId::new(bicycle)
            .ok_or_else(|| io::Error::other("invalid bicycle test identity"))?,
        to_fleet_id: FleetId::new(DESTINATION_FLEET_ID)
            .ok_or_else(|| io::Error::other("invalid destination fleet identity"))?,
    })
}

fn same_fleet_transfer_command(bicycle: &str) -> TestResult<TransferBicycle> {
    Ok(TransferBicycle {
        from_fleet_id: FleetId::new("city-fleet")
            .ok_or_else(|| io::Error::other("invalid source fleet identity"))?,
        bicycle_id: BicycleId::new(bicycle)
            .ok_or_else(|| io::Error::other("invalid bicycle test identity"))?,
        to_fleet_id: FleetId::new(demo_stream().aggregate_id().as_str())
            .ok_or_else(|| io::Error::other("invalid source fleet identity"))?,
    })
}

fn destination_stream() -> TestResult<rostfrei::StreamId> {
    rental_fleet::stream_id(DESTINATION_FLEET_ID).map_err(Into::into)
}

async fn cleanup_runtime_resources(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
) -> TestResult {
    let topology = config.messaging().topology();
    for stream in [
        topology.command_stream().as_str(),
        topology.command_response_stream().as_str(),
        topology.integration_event_stream().as_str(),
        topology.quarantine_stream().as_str(),
        config.event_store().stream_name(),
    ] {
        connection.delete_stream_if_exists(stream).await?;
    }
    Ok(())
}

fn required_nats_url() -> TestResult<String> {
    let value = env::var("ROSTFREI_NATS_URL").map_err(|_| {
        io::Error::other("ROSTFREI_NATS_URL is required for the ignored real-NATS transfer tests")
    })?;
    if value.trim().is_empty() {
        return Err(io::Error::other("ROSTFREI_NATS_URL must not be empty").into());
    }
    Ok(value)
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
