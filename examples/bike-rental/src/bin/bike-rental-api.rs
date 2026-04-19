use std::{env, sync::Arc};

use bike_rental::{
    nats_runtime::{
        BikeRentalNatsConfig, DEFAULT_APPLICATION_NAME, NatsCommandDispatchAdapter,
        RentBicycleMessageHandler,
    },
    rental::RentBicycle,
    runtime::{control_plane_builder, seed_demo},
    ui,
};
use rostfrei::EventHistory;
use rostfrei_control_plane::{
    ExposeTracePayloadsForLocalDevelopment,
    http::{self, DispatchHttpConfig, HttpConfig},
};
use rostfrei_messaging_core::{
    CommandPublisher, CommandResponsePublisher, CommandResponseReader, MessageConsumerFactory as _,
    MessageHandler,
};
use rostfrei_nats::{NatsConnectionConfig, ServerVersion, connect};

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

    let publisher = Arc::new(connection.publisher(nats.messaging().topology().clone()));
    let command_publisher: Arc<dyn CommandPublisher> = publisher.clone();
    let response_publisher: Arc<dyn CommandResponsePublisher> = publisher;
    let response_reader: Arc<dyn CommandResponseReader> =
        Arc::new(connection.command_response_reader(nats.messaging().topology().clone()));
    let dispatch_adapter = Arc::new(NatsCommandDispatchAdapter::new(
        command_publisher,
        Arc::clone(&response_reader),
        nats.command_address().clone(),
    ));
    let history: Arc<dyn EventHistory> = Arc::new(store.clone());
    let mut builder = control_plane_builder(history)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<RentBicycle>()?;
    builder.register_dispatch::<RentBicycle>(dispatch_adapter)?;
    let control_plane = builder.build()?;

    let consumer = connection
        .consumer_factory(nats.messaging().topology().clone())
        .create(nats.command_consumer().clone())?;
    let handler: Arc<dyn MessageHandler<_>> = Arc::new(RentBicycleMessageHandler::new(
        store,
        response_publisher,
        response_reader,
    ));

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
        result = consumer.run(handler) => match result {
            Ok(()) => return Err("bike-rental NATS command consumer stopped".into()),
            Err(error) => return Err(error.into()),
        },
    }
    Ok(())
}
