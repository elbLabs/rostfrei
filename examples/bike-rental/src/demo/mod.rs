use rostfrei::fixtures::{
    Fixture, FixtureApplyError, FixtureApplyReport, FixtureCodecRegistrationError,
    MessageSeriesEngine,
};
use rostfrei::{
    Command, CommandExecutionError, CommandExecutionMetadata, CommandExecutor, CommandOutcome,
    CommandReceipt, ContentFingerprint, ExecutionStore, FixtureEventStore, JsonCommandPayload,
    JsonRejectionCodec, OperationId, StreamId,
};
use thiserror::Error;

use crate::rental_fleet::{
    AddBicycle, AddBicycleHandler, BicycleCondition, BicycleId, FleetId, RentalFleetAggregate,
};

pub const DEMO_FLEET_ID: &str = "city-fleet";

const DEMO_FLEET_FIXTURE: &[u8] = include_bytes!("../../fixtures/demo-fleet.json");
const NO_RENTAL_FLEETS_FIXTURE: &[u8] = include_bytes!("../../fixtures/no-rental-fleets.json");
const RENTED_DEMO_FLEET_FIXTURE: &[u8] = include_bytes!("../../fixtures/rented-demo-fleet.json");
const RENTAL_WORKSHOP_FIXTURE: &[u8] = include_bytes!("../../fixtures/rental-workshop.json");
const RESERVABLE_BRANCH_FIXTURE: &[u8] = include_bytes!("../../fixtures/reservable-branch.json");
const BOOKED_CITY_BIKE_FIXTURE: &[u8] = include_bytes!("../../fixtures/booked-city-bike.json");
const RETURNED_CITY_BIKE_FIXTURE: &[u8] = include_bytes!("../../fixtures/returned-city-bike.json");
const PENDING_CITY_COLLECTION_FIXTURE: &[u8] =
    include_bytes!("../../fixtures/pending-city-collection.json");

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

pub fn rental_workshop_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(RENTAL_WORKSHOP_FIXTURE).map_err(Into::into)
}

pub fn reservable_branch_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(RESERVABLE_BRANCH_FIXTURE).map_err(Into::into)
}

pub fn booked_city_bike_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(BOOKED_CITY_BIKE_FIXTURE).map_err(Into::into)
}

pub fn returned_city_bike_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(RETURNED_CITY_BIKE_FIXTURE).map_err(Into::into)
}

pub fn pending_city_collection_fixture() -> Result<Fixture, DemoFixtureError> {
    serde_json::from_slice(PENDING_CITY_COLLECTION_FIXTURE).map_err(Into::into)
}

pub fn message_series_engine() -> Result<MessageSeriesEngine, DemoFixtureError> {
    let mut engine = MessageSeriesEngine::new();
    engine.register_json::<RentalFleetAggregate>()?;
    engine.register_json::<crate::repair_job::RepairJobAggregate>()?;
    engine.register_json::<crate::rental_tariff::RentalTariffAggregate>()?;
    engine.register_json::<crate::rental_payment::RentalPaymentAggregate>()?;
    Ok(engine)
}

pub async fn apply_fixture(
    store: &dyn FixtureEventStore,
    fixture: &Fixture,
) -> Result<FixtureApplyReport, DemoFixtureError> {
    message_series_engine()?
        .apply(store, fixture)
        .await
        .map_err(Into::into)
}

pub async fn apply_demo_fixture(
    store: &dyn FixtureEventStore,
) -> Result<FixtureApplyReport, DemoFixtureError> {
    apply_fixture(store, &demo_fixture()?).await
}

#[derive(Debug, Error)]
pub enum DemoSeedError {
    #[error("invalid demo identity: {0}")]
    InvalidIdentity(&'static str),
    #[error(transparent)]
    Identity(#[from] rostfrei::core::IdentityError),
    #[error(transparent)]
    Contract(#[from] rostfrei::messaging::ContractError),
    #[error(transparent)]
    Payload(#[from] serde_json::Error),
    #[error("invalid demo command payload: {0}")]
    CommandPayload(String),
    #[error(transparent)]
    Execution(#[from] CommandExecutionError),
    #[error("demo bicycle `{0}` already belongs to the fleet under another operation")]
    Rejected(&'static str),
}

/// Add the Normal demo fleet through the application's command handler. Repeated
/// startup replays the same durable decisions, including after later fleet changes.
pub async fn seed_demo<S: ExecutionStore + Clone>(
    store: &S,
) -> Result<Vec<CommandReceipt>, DemoSeedError> {
    let executor = CommandExecutor::new(store.clone());
    let mut receipts = Vec::new();
    for (bicycle, condition) in [
        ("bike-42", BicycleCondition::Serviceable),
        ("bike-99", BicycleCondition::MaintenanceRequired),
    ] {
        let command = AddBicycle {
            fleet_id: FleetId::new(DEMO_FLEET_ID)
                .ok_or(DemoSeedError::InvalidIdentity(DEMO_FLEET_ID))?,
            bicycle_id: BicycleId::new(bicycle).ok_or(DemoSeedError::InvalidIdentity(bicycle))?,
            condition,
            category: None,
        };
        let mut payload = command
            .encode_json()
            .map_err(DemoSeedError::CommandPayload)?;
        // Existing seed operation identities used payloads without the optional category.
        if let Some(fields) = payload.as_object_mut() {
            fields.remove("category");
        }
        let operation = format!("demo:add-bicycle:{DEMO_FLEET_ID}:{bicycle}");
        let metadata = CommandExecutionMetadata::new(
            OperationId::new(&operation)?,
            ContentFingerprint::digest(serde_json::to_vec(&(
                crate::BOUNDED_CONTEXT_NAME,
                AddBicycle::LOCAL_ID,
                AddBicycle::SCHEMA_VERSION,
                payload,
            ))?),
        )
        .with_bounded_context(rostfrei::BoundedContextName::new(
            crate::BOUNDED_CONTEXT_NAME,
        )?)
        .with_correlation_id(rostfrei::CorrelationId::new(operation)?);
        match executor
            .execute_with_rejection_codec(
                &AddBicycleHandler,
                metadata,
                &command,
                &JsonRejectionCodec::new("bike-rental.bicycle-already-in-fleet", 1),
            )
            .await?
        {
            CommandOutcome::Accepted(receipt) => receipts.push(receipt),
            CommandOutcome::Rejected(_) => return Err(DemoSeedError::Rejected(bicycle)),
        }
    }
    Ok(receipts)
}
