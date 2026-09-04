use std::env;

use rostfrei_nats::{NatsConnectionConfig, ServerVersion, connect};

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package_name = env!("CARGO_PKG_NAME");
    let nats_url = env::var("ROSTFREI_NATS_URL").unwrap_or_else(|_| DEFAULT_NATS_URL.to_owned());
    let config = NatsConnectionConfig::new(format!("{package_name}-application"), nats_url)
        .with_minimum_server_version(ServerVersion::new(2, 12, 1));
    let connection = connect(&config).await?;

    println!("{package_name} connected to NATS");
    tokio::signal::ctrl_c().await?;
    connection.drain().await?;

    Ok(())
}
