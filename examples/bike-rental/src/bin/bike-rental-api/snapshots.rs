use std::{error::Error, sync::Arc};

use bike_rental::{rental_fleet::RentalFleetAggregate, tracer};
use rostfrei::{AggregateCodecs, ApplicationName, SnapshotPolicy};
use rostfrei_nats::{NatsConnection, NatsReadModelConfig, NatsSnapshotStore, provision_read_model};
use rostfrei_tracer::{ApplicationBindings, Fixture, FixtureAggregate, FixtureSnapshot};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

pub async fn bindings(
    connection: &NatsConnection,
    application: &str,
    test: bool,
    enabled: bool,
) -> Result<Arc<ApplicationBindings>> {
    let mut bindings = tracer::bindings()?;
    if enabled {
        let application = ApplicationName::new(application)?;
        let context = if test {
            application.test_bounded_context("bike-rental")?
        } else {
            application.bounded_context("bike-rental")?
        };
        let config = NatsReadModelConfig::new(&context, "aggregate-snapshots")?;
        provision_read_model(connection.jetstream(), &config).await?;
        let snapshots = NatsSnapshotStore::connect(connection.jetstream().clone(), config).await?;
        let mut codecs = AggregateCodecs::default().with_snapshot_store(Arc::new(snapshots));
        codecs.register_json_snapshot::<RentalFleetAggregate>(
            "rental-fleet-v1",
            1,
            SnapshotPolicy::Manual,
        )?;
        bindings = bindings.with_event_codecs(Arc::new(codecs));
    }
    Ok(Arc::new(bindings))
}

pub fn fixture(fixture: Fixture, enabled: bool) -> Result<Fixture> {
    if !enabled {
        return Ok(fixture);
    }
    // Both populated demo fixtures contain these two original bicycle-added events.
    // The rented fixture also retains its later bicycle-rented event as the tail.
    Ok(fixture.with_snapshots(vec![FixtureSnapshot {
        aggregate: FixtureAggregate::new("bike-rental/rental-fleet", "city-fleet")?,
        stream_version: "2".into(),
        state: None,
    }])?)
}
