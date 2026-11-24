use bike_rental::{
    APPLICATION_NAME, BikeRentalNatsResourceLimits,
    demo::{
        booked_city_bike_fixture, collected_city_bike_fixture, demo_fixture,
        no_rental_fleets_fixture, pending_city_collection_fixture, rental_workshop_fixture,
        rented_demo_fixture, requested_rental_payment_fixture, reservable_branch_fixture,
        returned_city_bike_fixture, seed_demo,
    },
    tracer,
};
use rostfrei::nats::{NatsConnectionConfig, ServerVersion, connect};
use rostfrei::tracer::{
    ExposeTracePayloadsForLocalDevelopment, FilesystemTestRepository, ObservationFeed,
    ObservationScope, TracerConfig, http::HttpConfig,
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
    let epoch = match env::var("ROSTFREI_COMPLETED_OUTCOME_EPOCH") {
        Ok(epoch) => Some(epoch),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let configurations = |scope| {
        let configs = tracer::configurations(&application, scope, limits)?;
        match &epoch {
            Some(epoch) => configs.with_fresh_completed_outcome_epoch(epoch),
            None => configs.with_fresh_outcomes(),
        }
    };
    let normal = configurations(rostfrei::TrafficScope::Normal)?;
    let host = if enabled {
        let test = configurations(rostfrei::TrafficScope::Test)?;
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
            .with_continuous_observation(ObservationFeed::new(
                test.rental().application().clone(),
                ObservationScope::Test,
            ))
            .with_continuous_observation(ObservationFeed::new(
                normal.rental().application().clone(),
                ObservationScope::Production,
            ))
            .with_test_repository(Arc::new(FilesystemTestRepository::load(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/tracer"),
            )?))
            .with_trace_payload_policy(Arc::new(ExposeTracePayloadsForLocalDevelopment));
        let tracer = normal.with_dispatch_runtimes(&connection, tracer).await?;
        seed_demo(&normal.rental().connect_store(&connection).await?).await?;
        let host = test
            .application_builder(connection.clone())?
            .with_default_test_fixture(demo_fixture()?)
            .with_test_fixture(no_rental_fleets_fixture()?)
            .with_test_fixture(rented_demo_fixture()?)
            .with_test_fixture(rental_workshop_fixture()?)
            .with_test_fixture(reservable_branch_fixture()?)
            .with_test_fixture(booked_city_bike_fixture()?)
            .with_test_fixture(collected_city_bike_fixture()?)
            .with_test_fixture(returned_city_bike_fixture()?)
            .with_test_fixture(pending_city_collection_fixture()?)
            .with_test_fixture(requested_rental_payment_fixture()?)
            .with_tracer(Some(tracer))
            .provision_and_start()
            .await?;
        println!("bike-rental tracer listening on http://{address}");
        host
    } else {
        normal.provision(&connection).await?;
        seed_demo(&normal.rental().connect_store(&connection).await?).await?;
        normal
            .application_builder(connection.clone())?
            .connect_and_start()
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
