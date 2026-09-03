use std::env;

use bike_rental::{
    APPLICATION_NAME, BikeRentalNatsConfig, BikeRentalNatsResourceLimits,
    demo::{apply_fixture, demo_fixture, has_legacy_demo_seed},
};
use rostfrei_nats::{NatsConnectionConfig, ServerVersion, connect};

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let nats_url = env::var("ROSTFREI_NATS_URL").unwrap_or_else(|_| DEFAULT_NATS_URL.to_owned());
    let application =
        env::var("ROSTFREI_APPLICATION").unwrap_or_else(|_| APPLICATION_NAME.to_owned());
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let config = BikeRentalNatsConfig::new_with_resource_limits(&application, resource_limits)?;
    let connection = connect(
        &NatsConnectionConfig::new(format!("{application}-provision"), nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    config.provision(&connection).await?;
    let store = config.connect_store(&connection).await?;
    let fixture = demo_fixture()?;
    let legacy_seed_preserved = has_legacy_demo_seed(&store).await?;
    if !legacy_seed_preserved {
        apply_fixture(&store, &fixture).await?;
    }
    connection.drain().await?;

    if legacy_seed_preserved {
        println!(
            "provisioned NATS application `{application}` with its preserved legacy demo seed"
        );
    } else {
        println!(
            "provisioned NATS application `{application}` with fixture `{}`",
            fixture.id()
        );
    }
    Ok(())
}
