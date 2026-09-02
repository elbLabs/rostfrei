mod add_bicycle;
mod assess_rental_eligibility;
mod bicycle;
mod bicycle_availability;
mod import_rental_fleet;
mod rent_bicycle;
mod return_bicycle;

use rostfrei::{
    Aggregate, AggregateInstance, DomainIdentity, Entity, Initialize, StreamAggregateId,
    StreamAggregateType, StreamId, domain_actions,
};
use serde::{Deserialize, Serialize};

use super::BikeRental;
use assess_rental_eligibility::{RentalEligibilityDecisions, RentalEligibilityOutcome};

pub use add_bicycle::{AddBicycle, BicycleAdded};
pub use bicycle::{Bicycle, BicycleCondition, BicycleId, BicycleStatus};
pub use bicycle_availability::BicycleAvailability;
pub(crate) use bicycle_availability::BicycleAvailabilityQueries;
pub use import_rental_fleet::{ImportRentalFleetInput, ImportedBicycle, RentalFleetImported};
pub use rent_bicycle::{BicycleRented, BicycleUnavailable, RentBicycle};
pub use return_bicycle::{BicycleNotRented, BicycleReturned, ReturnBicycle};

#[derive(Aggregate)]
#[domain(
    id = "rental-fleet",
    label = "Rental fleet",
    context = BikeRental,
    root = RentalFleet,
    actions = [RentalFleetActionContract],
    decisions = [RentalEligibilityDecisions],
    events = [RentalFleetImported, BicycleAdded, BicycleRented, BicycleReturned]
)]
pub struct RentalFleetAggregate;

pub fn stream_id(aggregate_id: &str) -> Result<StreamId, &'static str> {
    let aggregate_type = StreamAggregateType::new(RentalFleetAggregate::aggregate_type())
        .map_err(|_| "invalid rental fleet aggregate type")?;
    let aggregate_id =
        StreamAggregateId::new(aggregate_id).map_err(|_| "invalid rental fleet ID")?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}

#[derive(Entity, Debug)]
#[domain(
    id = "rental-fleet-root",
    label = "Rental fleet",
    owner = RentalFleetAggregate
)]
pub struct RentalFleet {
    #[domain(identity)]
    fleet_id: FleetId,
    #[domain(entity)]
    bicycles: Vec<Bicycle>,
}

impl RentalFleet {
    pub const fn new(fleet_id: FleetId, bicycles: Vec<Bicycle>) -> Self {
        Self { fleet_id, bicycles }
    }

    pub fn bicycles(&self) -> &[Bicycle] {
        &self.bicycles
    }

    pub const fn fleet_id(&self) -> &FleetId {
        &self.fleet_id
    }
}

#[derive(
    DomainIdentity, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize,
)]
#[domain(owner = RentalFleet)]
#[serde(try_from = "String")]
pub struct FleetId(String);

impl FleetId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.trim().is_empty() && value.trim() == value).then_some(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&StreamAggregateId> for FleetId {
    fn from(value: &StreamAggregateId) -> Self {
        Self(value.as_str().to_owned())
    }
}

impl TryFrom<String> for FleetId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("fleet ID must be non-empty and trimmed")
    }
}

impl Initialize<RentalFleetAggregate> for RentalFleet {
    fn initialize(stream_id: &StreamId) -> Self {
        Self::new(FleetId::from(stream_id.aggregate_id()), Vec::new())
    }
}

#[domain_actions(aggregate(instance = RentalFleetActions))]
pub trait RentalFleetActionContract {
    #[action(
        id = "import-rental-fleet",
        label = "Import rental fleet",
        raises = [RentalFleetImported]
    )]
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput);

    #[action(
        id = "rent-bicycle",
        label = "Rent bicycle",
        raises = [BicycleRented]
    )]
    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable>;

    #[action(
        id = "return-bicycle",
        label = "Return bicycle",
        raises = [BicycleReturned]
    )]
    fn return_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleNotRented>;

    #[action(id = "add-bicycle", label = "Add bicycle", raises = [BicycleAdded])]
    fn add_bicycle(&mut self);
}

impl RentalFleetActions for AggregateInstance<RentalFleetAggregate> {
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput) {
        self.raise(RentalFleetImported {
            fleet_id: self.state().fleet_id.clone(),
            bicycles: input.into_bicycles(),
        });
    }

    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable> {
        let event = {
            let root = self.state();
            let bicycle = root
                .bicycles
                .iter()
                .find(|bicycle| bicycle.bicycle_id() == &input)
                .ok_or_else(|| BicycleUnavailable {
                    bicycle_id: input.clone(),
                })?;
            match RentalFleetAggregate::assess_rental_eligibility(
                bicycle.status(),
                bicycle.condition(),
            ) {
                RentalEligibilityOutcome::Eligible => BicycleRented {
                    fleet_id: root.fleet_id.clone(),
                    bicycle_id: input.clone(),
                },
                RentalEligibilityOutcome::AlreadyRented
                | RentalEligibilityOutcome::MaintenanceRequired => {
                    return Err(BicycleUnavailable { bicycle_id: input });
                }
            }
        };

        self.raise(event);
        Ok(())
    }

    fn return_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleNotRented> {
        let root = self.state();
        let rented = root.bicycles.iter().any(|bicycle| {
            bicycle.bicycle_id() == &input && bicycle.status() == BicycleStatus::Rented
        });
        if !rented {
            return Err(BicycleNotRented { bicycle_id: input });
        }
        let fleet_id = root.fleet_id.clone();
        self.raise(BicycleReturned {
            fleet_id,
            bicycle_id: input,
        });
        Ok(())
    }

    fn add_bicycle(&mut self) {
        let fleet_id = self.state().fleet_id.clone();
        let bicycle_id = add_bicycle::allocate_bicycle_id(self.state());
        self.raise(BicycleAdded {
            fleet_id,
            bicycle_id,
            condition: BicycleCondition::Serviceable,
        });
    }
}
