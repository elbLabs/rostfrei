use std::{env, sync::Arc};

use bike_rental::{
    nats_runtime::{
        BicycleRentalStartedHandler, BicycleRentedIntegrationMapper, BikeRentalNatsConfig,
        DEFAULT_APPLICATION_NAME,
    },
    rental::{BicycleRented, RentBicycle, RentalFleetAggregate},
    runtime::{control_plane_builder, seed_demo},
    ui,
};
use rostfrei::{
    CommandBus, CommandMessageAdapter, CommandProcessor, DomainEventDefinitionType,
    DomainEventDispatcher, EventHistory, EventStore, IntegrationEventBus,
    IntegrationMessageAdapter, JsonDomainRejectionMapper,
};
use rostfrei_control_plane::{
    CommandBusDispatchAdapter, ExposeTracePayloadsForLocalDevelopment,
    http::{self, DispatchHttpConfig, HttpConfig},
};
use rostfrei_messaging_core::{
    CommandRejectionClassification, MessageConsumerFactory as _, MessageHandler,
};
use rostfrei_nats::{NatsConnectionConfig, NatsDomainEventConsumer, ServerVersion, connect};

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let application =
        env::var("ROSTFREI_APPLICATION").unwrap_or_else(|_| DEFAULT_APPLICATION_NAME.to_owned());
    let nats_url = env::var("ROSTFREI_NATS_URL").unwrap_or_else(|_| DEFAULT_NATS_URL.to_owned());
    let nats = BikeRentalNatsConfig::new(&application)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{application}-api"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 0)),
    )
    .await?;
    connection
        .verify_application_messaging(nats.messaging())
        .await?;
    let store = nats.connect_store(&connection).await?;
    seed_demo(&store).await?;

    let messaging = Arc::new(connection.messaging_adapter(nats.messaging().topology().clone()));
    let event_store: Arc<dyn EventStore> = Arc::new(store.clone());
    let mut processor = CommandProcessor::new(event_store);
    processor.register::<RentBicycle, _>(JsonDomainRejectionMapper::new(
        CommandRejectionClassification::Conflict,
    ))?;
    let processor = Arc::new(processor);
    let command_adapter: Arc<dyn CommandMessageAdapter> = messaging.clone();
    let command_bus = CommandBus::new(nats.context().clone(), command_adapter);
    let dispatch_adapter = Arc::new(CommandBusDispatchAdapter::new(command_bus));
    let history: Arc<dyn EventHistory> = Arc::new(store.clone());
    let mut builder = control_plane_builder(history)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<RentBicycle>()?;
    builder.register_dispatch::<RentBicycle>(dispatch_adapter)?;
    let control_plane = builder.build()?;

    let command_consumer = connection
        .consumer_factory(nats.messaging().topology().clone())
        .create(nats.command_consumer().clone())?;
    let command_handler: Arc<dyn MessageHandler<_>> =
        Arc::new(messaging.command_handler(processor));

    let integration_adapter: Arc<dyn IntegrationMessageAdapter> = messaging;
    let integration_bus = IntegrationEventBus::new(nats.context().clone(), integration_adapter);
    let mut dispatcher = DomainEventDispatcher::new();
    dispatcher.register::<RentalFleetAggregate, BicycleRented, _>(
        BicycleRented::DEFINITION.id,
        Arc::new(BicycleRentedIntegrationMapper::new(integration_bus)),
    )?;
    let domain_event_consumer = NatsDomainEventConsumer::connect(
        connection.jetstream().clone(),
        nats.event_store().clone(),
        nats.domain_event_consumer().clone(),
        Arc::new(dispatcher),
    )
    .await?;
    let integration_event_consumer = connection
        .consumer_factory(nats.messaging().topology().clone())
        .create(nats.integration_event_consumer().clone())?;
    let integration_event_handler: Arc<dyn MessageHandler<_>> =
        Arc::new(BicycleRentalStartedHandler);
    let (_domain_shutdown, domain_shutdown_receiver) = tokio::sync::watch::channel(false);

    let api_token = env::var("ROSTFREI_API_TOKEN")?;
    let dispatch_token = env::var("ROSTFREI_DISPATCH_TOKEN")?;
    let app = ui::router()
        .merge(http::router(
            control_plane.clone(),
            HttpConfig::new(api_token)?,
        ))
        .merge(http::dispatch_router(
            control_plane,
            DispatchHttpConfig::new(dispatch_token)?,
        ));

    let address = env::var("ROSTFREI_API_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("bike-rental control plane listening on http://{address}");
    tokio::select! {
        result = axum::serve(listener, app) => result?,
        result = command_consumer.run(command_handler) => match result {
            Ok(()) => return Err("bike-rental NATS command consumer stopped".into()),
            Err(error) => return Err(error.into()),
        },
        result = domain_event_consumer.run_until_shutdown(domain_shutdown_receiver) => match result {
            Ok(()) => return Err("bike-rental NATS domain-event consumer stopped".into()),
            Err(error) => return Err(error.into()),
        },
        result = integration_event_consumer.run(integration_event_handler) => match result {
            Ok(()) => return Err("bike-rental NATS integration-event consumer stopped".into()),
            Err(error) => return Err(error.into()),
        },
    }
    Ok(())
}
