use rostfrei::{EventStore, StreamId};
use rostfrei_fixtures::{
    Fixture, FixtureApplyError, FixtureApplyReport, FixtureCodecRegistrationError,
    MessageSeriesEngine,
};
use thiserror::Error;

use crate::rental_fleet::RentalFleetAggregate;

pub const DEMO_FLEET_ID: &str = "city-fleet";

const DEMO_FLEET_FIXTURE: &[u8] = include_bytes!("../../fixtures/demo-fleet.json");
const NO_RENTAL_FLEETS_FIXTURE: &[u8] = include_bytes!("../../fixtures/no-rental-fleets.json");
const RENTED_DEMO_FLEET_FIXTURE: &[u8] = include_bytes!("../../fixtures/rented-demo-fleet.json");

#[allow(
    clippy::expect_used,
    reason = "the compiled demo aggregate and fixture identities are valid by construction"
)]
pub fn demo_stream() -> StreamId {
    crate::rental_fleet::stream_id(DEMO_FLEET_ID).expect("static demo stream identity is valid")
}

#[derive(Debug, Error)]
pub enum DemoFixtureError {
    #[error("bike-rental fixture document is invalid: {0}")]
    Document(#[from] serde_json::Error),
    #[error(transparent)]
    Registration(#[from] FixtureCodecRegistrationError),
    #[error(transparent)]
    Apply(#[from] FixtureApplyError),
}

pub fn demo_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(DEMO_FLEET_FIXTURE).map_err(Into::into)
}

pub fn rented_demo_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(RENTED_DEMO_FLEET_FIXTURE).map_err(Into::into)
}

pub fn no_rental_fleets_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(NO_RENTAL_FLEETS_FIXTURE).map_err(Into::into)
}

pub fn message_series_engine() -> Result<MessageSeriesEngine, DemoFixtureError> {
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<RentalFleetAggregate>()?;
    Ok(engine)
}

pub async fn apply_fixture(
    store: &dyn EventStore,
    fixture: &Fixture,
) -> Result<FixtureApplyReport, DemoFixtureError> {
    message_series_engine()?
        .apply(store, fixture)
        .await
        .map_err(Into::into)
}

pub async fn apply_demo_fixture(
    store: &dyn EventStore,
) -> Result<FixtureApplyReport, DemoFixtureError> {
    apply_fixture(store, &demo_fixture()?).await
}
