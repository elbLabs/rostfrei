//! An inspection-only host: connects to existing resources and runs independently of workers.
use std::{env, sync::Arc};

use rostfrei::{DomainRegistry, InMemoryEventStore};
use rostfrei_messaging_core::ApplicationName;
use rostfrei_nats::{MessagingTopology, NatsConnectionConfig, NatsQuarantineReader, connect};
use rostfrei_tracer::{
    TracerBuilder,
    http::{HttpConfig, router},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = connect(&NatsConnectionConfig::new(
        "quarantine-inspector",
        env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(
        env::var("ROSTFREI_APPLICATION")
            .unwrap_or_else(|_| bike_rental::APPLICATION_NAME.to_owned()),
    )?;
    let reader = Arc::new(NatsQuarantineReader::new(
        connection.jetstream().clone(),
        MessagingTopology::for_application(&application)?,
    ));
    // Tracer's builder requires history; an empty catalog/history installs no executable commands.
    let tracer = TracerBuilder::new(Arc::new(InMemoryEventStore::new()), DomainRegistry::new())
        .with_production_quarantine_reader(reader)
        .build()?;
    let config = HttpConfig::inspection_only(env::var("ROSTFREI_INSPECTION_TOKEN")?)?;
    let address =
        env::var("ROSTFREI_INSPECTION_ADDR").unwrap_or_else(|_| "127.0.0.1:1310".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("quarantine inspection listening on http://{address}");
    axum::serve(listener, router(tracer, config)).await?;
    Ok(())
}
