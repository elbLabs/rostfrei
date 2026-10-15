use rostfrei::ApplicationName;
use rostfrei_nats::{NatsConnectionConfig, connect};
use rostfrei_tracer::{
    ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository, NatsApplication,
    TracerConfig, http::HttpConfig,
};
use std::{env, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let connection = connect(&NatsConnectionConfig::new(
        "counter-dev",
        env::var("ROSTFREI_NATS_URL")?,
    ))
    .await?;
    let application = ApplicationName::new(env::var("ROSTFREI_APPLICATION")?)?;
    let listener = tokio::net::TcpListener::bind(
        env::var("ROSTFREI_API_ADDR").unwrap_or_else(|_| "127.0.0.1:1309".into()),
    )
    .await?;
    let config = TracerConfig::new(listener, HttpConfig::new(env::var("ROSTFREI_API_TOKEN")?)?)
        .with_test_repository(Arc::new(FilesystemTestRepository::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests"
        ))?))
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    let host = NatsApplication::builder(
        application.test_bounded_context("counter")?,
        connection.clone(),
        tracer_host_consumer::bindings()?,
    )?
    .with_default_test_fixture(serde_json::from_str(include_str!("../fixture.json"))?)
    .with_tracer(Some(config))
    .provision_and_start()
    .await?;
    let result = host
        .run_until(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    connection.drain().await?;
    Ok(result?)
}
