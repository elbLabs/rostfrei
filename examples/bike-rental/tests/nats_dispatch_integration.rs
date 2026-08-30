use std::{
    env,
    error::Error,
    io, process,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use bike_rental::{
    nats_runtime::{BicycleRentalStarted, BicycleRentedIntegrationMapper, BikeRentalNatsConfig},
    rental::{BicycleRented, RentBicycle, RentalFleetAggregate},
    runtime::{control_plane_builder, demo_stream, seed_demo},
};
use rostfrei::{
    CommandBus, CommandMessageAdapter, CommandProcessor, DomainEventDefinitionType,
    DomainEventDispatcher, EncodedIntegrationMessage, EventHistory, EventStore,
    IntegrationEventBus, IntegrationMessageAdapter, JsonDomainRejectionMapper,
};
use rostfrei_control_plane::{
    CommandBusDispatchAdapter, ControlPlane, DispatchRequest, OperationResult, SimulationRequest,
};
use rostfrei_messaging_core::{
    CommandRejectionClassification, CommandResponse, CommandResponseOutcome, DeliveryDisposition,
    IntegrationEventAddress, IntegrationEventEnvelope, MessageConsumerFactory as _,
    MessageDelivery, MessageHandler,
};
use rostfrei_nats::{
    NatsConnection, NatsConnectionConfig, NatsDomainEventConsumer, ServerVersion, connect,
};
use serde_json::json;
use tokio::{
    sync::mpsc,
    task::JoinHandle,
    time::{Instant, sleep},
};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct IntegrationRecorder {
    sender: mpsc::Sender<IntegrationEventEnvelope<BicycleRentalStarted>>,
}

#[async_trait::async_trait]
impl MessageHandler<IntegrationEventAddress> for IntegrationRecorder {
    async fn handle(
        &self,
        delivery: MessageDelivery<IntegrationEventAddress>,
    ) -> DeliveryDisposition {
        let envelope = EncodedIntegrationMessage::from_delivery(
            delivery.address().clone(),
            delivery.message_id().clone(),
            delivery.payload().to_vec(),
        )
        .and_then(|message| message.decode::<BicycleRentalStarted>());
        let Ok(envelope) = envelope else {
            return DeliveryDisposition::Terminate;
        };
        if self.sender.send(envelope).await.is_err() {
            return DeliveryDisposition::Terminate;
        }
        DeliveryDisposition::Acknowledge
    }
}

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

#[allow(clippy::too_many_lines)]
async fn run_dispatch_test(
    connection: &NatsConnection,
    config: &BikeRentalNatsConfig,
) -> TestResult {
    let store = config.connect_store(connection).await?;
    seed_demo(&store).await?;
    let messaging = Arc::new(connection.messaging_adapter(config.messaging().topology().clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(store.clone());
    let mut processor = CommandProcessor::new(event_store);
    processor.register::<RentBicycle, _>(JsonDomainRejectionMapper::new(
        CommandRejectionClassification::Conflict,
    ))?;
    let consumer = connection
        .consumer_factory(config.messaging().topology().clone())
        .create(config.command_consumer().clone())?;
    let handler: Arc<dyn MessageHandler<_>> =
        Arc::new(messaging.command_handler(Arc::new(processor)));
    let command_consumer_task = tokio::spawn(async move { consumer.run(handler).await });

    let integration_adapter: Arc<dyn IntegrationMessageAdapter> = messaging.clone();
    let integration_bus = IntegrationEventBus::new(config.context().clone(), integration_adapter);
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register::<RentalFleetAggregate, BicycleRented, _>(
        BicycleRented::DEFINITION.id,
        Arc::new(BicycleRentedIntegrationMapper::new(integration_bus)),
    )?;
    let domain_consumer = NatsDomainEventConsumer::connect(
        connection.jetstream().clone(),
        config.event_store().clone(),
        config.domain_event_consumer().clone(),
        Arc::new(dispatcher),
    )
    .await?;
    let (domain_shutdown, domain_shutdown_receiver) = tokio::sync::watch::channel(false);
    let domain_consumer_task = tokio::spawn(async move {
        domain_consumer
            .run_until_shutdown(domain_shutdown_receiver)
            .await
    });
    let integration_consumer = connection
        .consumer_factory(config.messaging().topology().clone())
        .create(config.integration_event_consumer().clone())?;
    let (integration_sender, mut integration_receiver) = mpsc::channel(1);
    let integration_handler: Arc<dyn MessageHandler<_>> = Arc::new(IntegrationRecorder {
        sender: integration_sender,
    });
    let integration_consumer_task =
        tokio::spawn(async move { integration_consumer.run(integration_handler).await });

    let test = async {
        let command_adapter: Arc<dyn CommandMessageAdapter> = messaging;
        let command_bus = CommandBus::new(config.context().clone(), command_adapter);
        let adapter = Arc::new(CommandBusDispatchAdapter::new(command_bus));
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
        let integration =
            tokio::time::timeout(Duration::from_secs(10), integration_receiver.recv())
                .await?
                .ok_or_else(|| io::Error::other("integration-event consumer stopped"))?;
        ensure(
            integration.payload().fleet_id().as_str() == "city-fleet"
                && integration.payload().bicycle_id().as_str() == "bike-42",
            "integration event did not preserve rental identity",
        )?;
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
    let _ = domain_shutdown.send(true);
    stop_task(command_consumer_task).await;
    stop_task(domain_consumer_task).await;
    stop_task(integration_consumer_task).await;
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

async fn stop_task<T>(task: JoinHandle<T>) {
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
