use std::{env, path::PathBuf, sync::Arc};

use bike_rental::{
    BikeRentalNatsResourceLimits, BikeRentalNatsRuntime, domain_model,
    rental_fleet::{AddBicycle, RentBicycle, RentalFleetAggregate, ReturnBicycle},
    tracer::{self, RentBicycleInputOptions, ReturnBicycleInputOptions},
};
use rostfrei::EventHistory;
use rostfrei_nats::{NatsConnectionConfig, ServerVersion, connect};
use rostfrei_tracer::{
    ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository, OperationMode,
    TestRepository, TestScenarioReset,
    http::{self, HttpConfig},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let nats_url = env::var("ROSTFREI_NATS_URL")?;
    let application = env::var("ROSTFREI_APPLICATION").unwrap_or_else(|_| "bike-rental".to_owned());
    let test_application = format!("{application}-test");
    let production_application = format!("{application}-prod");
    let resource_limits = BikeRentalNatsResourceLimits::from_env()?;
    let connection = connect(
        &NatsConnectionConfig::new("bike-rental-api", nats_url)
            .with_minimum_server_version(ServerVersion::new(2, 12, 1)),
    )
    .await?;

    let test_runtime = Arc::new(
        BikeRentalNatsRuntime::provision_with_resource_limits(
            connection.clone(),
            &test_application,
            resource_limits,
        )
        .await?,
    );
    test_runtime.reset().await?;

    let production_runtime = Arc::new(
        BikeRentalNatsRuntime::provision_with_resource_limits(
            connection,
            &production_application,
            resource_limits,
        )
        .await?,
    );
    production_runtime.seed_demo().await?;
    production_runtime.start_workers().await?;

    let test_store = Arc::new(test_runtime.store().clone());
    let history: Arc<dyn EventHistory> = test_store.clone();
    let test_reset: Arc<dyn TestScenarioReset> = test_runtime.clone();
    let test_repository: Arc<dyn TestRepository> = Arc::new(FilesystemTestRepository::load(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer"),
    )?);
    let mut builder = tracer::builder(history)?
        .with_domain_model(domain_model()?)
        .with_test_event_store(test_store.clone())
        .with_stream_directory(test_store)
        .with_test_transport(test_runtime.transport())
        .with_dispatch_transport(production_runtime.transport())
        .with_test_fixture("demo-fleet", test_reset)
        .with_test_repository(test_repository)
        .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
    builder.register_json::<RentalFleetAggregate, RentBicycle>()?;
    builder.register_json::<RentalFleetAggregate, ReturnBicycle>()?;
    builder.register_json::<RentalFleetAggregate, AddBicycle>()?;
    builder
        .register_input_options::<RentalFleetAggregate, RentBicycle, _>(RentBicycleInputOptions)?;
    builder.register_input_options::<RentalFleetAggregate, ReturnBicycle, _>(
        ReturnBicycleInputOptions,
    )?;
    let tracer = builder.build()?;
    let mut test_correlation_observer = test_runtime
        .start_correlation_observer(tracer.correlation_observer(OperationMode::Test))
        .await?;
    let mut production_correlation_observer = production_runtime
        .start_correlation_observer(tracer.correlation_observer(OperationMode::Dispatch))
        .await?;
    let api_token = env::var("ROSTFREI_API_TOKEN")?;
    let dispatch_token = env::var("ROSTFREI_DISPATCH_TOKEN")?;
    let http_config = HttpConfig::new(api_token)?.with_dispatch_token(dispatch_token)?;
    let app = http::router(tracer, http_config);

    let address = env::var("ROSTFREI_API_ADDR").unwrap_or_else(|_| "127.0.0.1:1309".to_owned());
    let listener = tokio::net::TcpListener::bind(&address).await?;
    println!("bike-rental tracer listening on http://{address}");
    tokio::select! {
        result = axum::serve(listener, app) => result.map_err(Into::into),
        () = test_runtime.wait_for_worker_exit() => {
            Err(std::io::Error::other("a test NATS worker stopped unexpectedly").into())
        }
        () = production_runtime.wait_for_worker_exit() => {
            Err(std::io::Error::other("a production NATS worker stopped unexpectedly").into())
        }
        result = &mut test_correlation_observer => {
            Err(std::io::Error::other(format!("the test correlation observer stopped: {result:?}")).into())
        }
        result = &mut production_correlation_observer => {
            Err(std::io::Error::other(format!("the production correlation observer stopped: {result:?}")).into())
        }
    }
}
