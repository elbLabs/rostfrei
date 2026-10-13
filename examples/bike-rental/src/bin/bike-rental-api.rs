use bike_rental::{
    APPLICATION_NAME, BikeRentalNatsConfig, BikeRentalNatsResourceLimits,
    demo::{demo_fixture, no_rental_fleets_fixture, rented_demo_fixture},
    tracer,
};
use rostfrei_nats::{NatsConnectionConfig, ServerVersion, connect};
use rostfrei_tracer::{
    ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository, NatsApplication, NatsRuntime,
    TracerConfig, http::HttpConfig,
};
use std::{env, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let enabled = env::args().skip(1).any(|argument| argument == "--tracer");
    let application =
        env::var("ROSTFREI_APPLICATION").unwrap_or_else(|_| APPLICATION_NAME.to_owned());
    let limits = BikeRentalNatsResourceLimits::from_env()?;
    let connection = connect(
        &NatsConnectionConfig::new("bike-rental", env::var("ROSTFREI_NATS_URL")?)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;
    let bindings = Arc::new(tracer::bindings()?);
    let normal = BikeRentalNatsConfig::for_bindings(
        &application,
        rostfrei::TrafficScope::Normal,
        limits,
        &bindings,
    )?;
    let host = if enabled {
        let test = BikeRentalNatsConfig::for_bindings(
            &application,
            rostfrei::TrafficScope::Test,
            limits,
            &bindings,
        )?;
        let dispatch = Arc::new(
            NatsRuntime::provision(
                connection.clone(),
                normal.host_config().clone(),
                bindings.clone(),
            )
            .await?,
        );
        dispatch.apply_fixture(&demo_fixture()?).await?;
        let address = env::var("ROSTFREI_API_ADDR").unwrap_or_else(|_| "127.0.0.1:1309".to_owned());
        let listener = tokio::net::TcpListener::bind(&address).await?;
        let mut http = HttpConfig::new(env::var("ROSTFREI_API_TOKEN")?)?
            .with_dispatch_token(env::var("ROSTFREI_DISPATCH_TOKEN")?)?;
        match env::var("ROSTFREI_INSPECTION_TOKEN") {
            Ok(token) => http = http.with_inspection_token(token)?,
            Err(env::VarError::NotPresent) => {}
            Err(error) => return Err(error.into()),
        }
        let tracer = TracerConfig::new(listener, http)
            .with_dispatch_runtime(dispatch)
            .with_test_repository(Arc::new(FilesystemTestRepository::load(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer"),
            )?))
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
        let host = NatsApplication::builder(test.context().clone(), connection.clone(), bindings)?
            .with_runtime_config(test.host_config().clone())
            .with_default_test_fixture(demo_fixture()?)
            .with_test_fixture(no_rental_fleets_fixture()?)
            .with_test_fixture(rented_demo_fixture()?)
            .with_tracer(Some(tracer))
            .provision_and_start()
            .await?;
        println!("bike-rental tracer listening on http://{address}");
        host
    } else {
        NatsApplication::builder(normal.context().clone(), connection.clone(), bindings)?
            .with_runtime_config(normal.host_config().clone())
            .with_initial_fixture(demo_fixture()?)
            .provision_and_start()
            .await?
    };
    let result = host
        .run_until(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    connection.drain().await?;
    result.map_err(Into::into)
}
