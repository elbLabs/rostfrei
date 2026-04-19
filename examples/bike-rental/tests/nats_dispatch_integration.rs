use std::{
    env,
    error::Error,
    io, process,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bike_rental::{
    nats_runtime::{BikeRentalNatsConfig, NatsCommandDispatchAdapter, RentBicycleMessageHandler},
    rental::RentBicycle,
    runtime::{control_plane_builder, demo_stream, seed_demo},
};
use rostfrei::EventHistory;
use rostfrei_control_plane::{ControlPlane, DispatchRequest, OperationResult, SimulationRequest};
use rostfrei_messaging_core::{
    CommandPublisher, CommandResponse, CommandResponseOutcome, CommandResponsePublisher,
    CommandResponseReader, MessageConsumerFactory as _, MessageHandler,
};
use rostfrei_nats::{NatsConnection, NatsConnectionConfig, ServerVersion, connect};
use serde_json::json;
use tokio::{
    task::JoinHandle,
    time::{Instant, sleep},
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn real_nats_dispatch_updates_the_stream_and_cleans_its_scope() -> TestResult {
    let Ok(nats_url) = env::var("ROSTFREI_NATS_URL") else {
        return Ok(());
    };
    let application = unique_application()?;
    let config = BikeRentalNatsConfig::new(&application)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{application}-test"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 0)),
    )
    .await?;
    config.provision(&connection).await?;

    let test = run_dispatch_test(&connection, &config).await;
    let cleanup = cleanup(&connection, &config).await;
    test?;
    cleanup?;
    Ok(())
}

async fn run_dispatch_test(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
) -> TestResult {
    let store = config.connect_store(connection).await?;
    seed_demo(&store).await?;
    let publisher = Arc::new(connection.publisher(config.messaging().topology().clone()));
    let response_publisher: Arc<dyn CommandResponsePublisher> = publisher.clone();
    let response_reader: Arc<dyn CommandResponseReader> =
        Arc::new(connection.command_response_reader(config.messaging().topology().clone()));
    let consumer = connection
        .consumer_factory(config.messaging().topology().clone())
        .create(config.command_consumer().clone())?;
    let handler: Arc<dyn MessageHandler<_>> = Arc::new(RentBicycleMessageHandler::new(
        store.clone(),
        response_publisher,
        Arc::clone(&response_reader),
    ));
    let consumer_task = tokio::spawn(async move { consumer.run(handler).await });

    let test = async {
        let command_publisher: Arc<dyn CommandPublisher> = publisher.clone();
        let adapter = Arc::new(NatsCommandDispatchAdapter::new(
            command_publisher,
            response_reader,
            config.command_address().clone(),
        ));
        let history: Arc<dyn EventHistory> = Arc::new(store.clone());
        let mut builder = control_plane_builder(history);
        builder.register_json::<RentBicycle>()?;
        builder.register_dispatch::<RentBicycle>(adapter)?;
        let control_plane = builder.build()?;

        let first = dispatch(&control_plane, "real-rental-first").await?;
        ensure(
            matches!(
                first,
                OperationResult::Accepted {
                    published: true,
                    ..
                }
            ),
            "first dispatch did not complete as accepted",
        )?;
        wait_for_history_len(&store, 2).await?;
        let second = dispatch(&control_plane, "real-rental-second").await?;
        ensure(
            matches!(
                second,
                OperationResult::Rejected {
                    published: true,
                    ..
                }
            ),
            "second dispatch did not complete as rejected",
        )?;
        wait_for_command_stream_empty(connection, config).await?;
        ensure(
            store.load(&demo_stream()?).await?.len() == 2,
            "the rejected second rental appended an event",
        )?;
        assert_retained_responses(connection, config).await?;

        control_plane
            .submit_simulation(
                "bike-rental/rental-fleet",
                "city-fleet",
                "rent-bicycle",
                SimulationRequest {
                    schema_version: 1,
                    payload: json!({ "bicycle_id": "bike-42" }),
                },
                Some("real-rental-inspection"),
            )
            .await?;
        let inspected = terminal_operation(&control_plane, "real-rental-inspection").await?;
        ensure(
            matches!(inspected.result, Some(OperationResult::Rejected { .. })),
            "simulation did not observe the persisted rental",
        )
    }
    .await;
    stop_consumer(consumer_task).await;
    test
}

async fn dispatch(control_plane: &ControlPlane, operation_id: &str) -> TestResult<OperationResult> {
    control_plane
        .submit_dispatch(
            "bike-rental/rental-fleet",
            "city-fleet",
            "rent-bicycle",
            DispatchRequest {
                schema_version: 1,
                payload: json!({ "bicycle_id": "bike-42" }),
            },
            operation_id,
        )
        .await?;
    let operation = terminal_operation(control_plane, operation_id).await?;
    operation
        .result
        .ok_or_else(|| io::Error::other("dispatch completed without a terminal result").into())
}

async fn assert_retained_responses(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
) -> TestResult {
    let mut stream = connection
        .jetstream()
        .get_stream(
            config
                .messaging()
                .topology()
                .command_response_stream()
                .as_str(),
        )
        .await?;
    let info = stream.info().await?;
    ensure(
        info.state.messages == 2,
        "response stream did not retain two outcomes",
    )?;
    let mut accepted = 0_u64;
    let mut rejected = 0_u64;
    for sequence in 1..=info.state.last_sequence {
        let stored = stream.get_raw_message(sequence).await?;
        let response: CommandResponse = serde_json::from_slice(&stored.payload)?;
        match response.outcome() {
            CommandResponseOutcome::Accepted => accepted = accepted.saturating_add(1),
            CommandResponseOutcome::Rejected(_) => rejected = rejected.saturating_add(1),
        }
    }
    ensure(
        accepted == 1 && rejected == 1,
        "response stream did not retain accepted and rejected outcomes",
    )
}

async fn terminal_operation(
    control_plane: &ControlPlane,
    operation_id: &str,
) -> TestResult<rostfrei_control_plane::OperationSnapshot> {
    let mut subscription = control_plane.subscribe(operation_id, 0).await?;
    while subscription.next().await.is_some() {}
    Ok(control_plane.operation(operation_id).await?)
}

async fn wait_for_history_len(
    store: &rostfrei_nats::NatsEventStore,
    expected: usize,
) -> TestResult {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or_else(|| io::Error::other("aggregate-history deadline overflowed"))?;
    loop {
        if store.load(&demo_stream()?).await?.len() == expected {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for aggregate history").into());
        }
        sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_command_stream_empty(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
) -> TestResult {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .ok_or_else(|| io::Error::other("command-stream deadline overflowed"))?;
    let stream_name = config.messaging().topology().command_stream().as_str();
    loop {
        let mut stream = connection.jetstream().get_stream(stream_name).await?;
        if stream.info().await?.state.messages == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for command acknowledgement").into());
        }
        sleep(Duration::from_millis(25)).await;
    }
}

async fn stop_consumer(task: JoinHandle<Result<(), rostfrei_messaging_core::ConsumeError>>) {
    task.abort();
    let _ = task.await;
}

async fn cleanup(connection: &NatsConnection, config: &BikeRentalNatsConfig) -> TestResult {
    let mut first_error = None;
    let stream_names = [
        config.messaging().topology().command_stream().as_str(),
        config
            .messaging()
            .topology()
            .command_response_stream()
            .as_str(),
        config
            .messaging()
            .topology()
            .integration_event_stream()
            .as_str(),
        config.messaging().topology().quarantine_stream().as_str(),
        config.event_store().stream_name(),
    ];
    for stream_name in stream_names {
        if let Err(error) = connection.jetstream().delete_stream(stream_name).await
            && first_error.is_none()
        {
            first_error = Some(error.to_string());
        }
    }
    if let Err(error) = connection.drain().await
        && first_error.is_none()
    {
        first_error = Some(error.to_string());
    }
    first_error.map_or_else(|| Ok(()), |error| Err(io::Error::other(error).into()))
}

fn unique_application() -> TestResult<String> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(format!("bike-rental-test-{:x}-{nanos:x}", process::id()))
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
