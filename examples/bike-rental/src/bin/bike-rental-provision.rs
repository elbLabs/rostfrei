use std::env;

use bike_rental::{nats_runtime::BikeRentalNatsConfig, runtime::seed_demo};
use rostfrei_nats::{NatsConnectionConfig, ServerVersion, connect};

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let nats_url = env::var("ROSTFREI_NATS_URL").unwrap_or_else(|_| DEFAULT_NATS_URL.to_owned());
    let application = env::var("ROSTFREI_APPLICATION").unwrap_or_else(|_| "bike-rental".to_owned());
    let production_application = format!("{application}-prod");
    let config = BikeRentalNatsConfig::new(&production_application)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{application}-provision"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    config.provision(&connection).await?;
    let store = config.connect_store(&connection).await?;
    seed_demo(&store).await?;
    connection.drain().await?;

    println!("provisioned and seeded NATS application `{production_application}`");
    Ok(())
}
