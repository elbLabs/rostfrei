use rostfrei::{Apply, DomainEvent, ValueObject};
use serde::{Deserialize, Serialize};

use super::{
    Bicycle, BicycleCondition, BicycleId, BicycleStatus, FleetId, RentalFleet, RentalFleetAggregate,
};

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "imported-bicycle",
    label = "Imported bicycle",
    owner = RentalFleetAggregate
)]
pub struct ImportedBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
    #[domain(value_object)]
    pub status: BicycleStatus,
    #[domain(value_object)]
    pub condition: BicycleCondition,
}

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "import-rental-fleet-input",
    label = "Import rental fleet input",
    owner = RentalFleetAggregate
)]
pub struct ImportRentalFleetInput {
    #[domain(value_object)]
    bicycles: Vec<ImportedBicycle>,
}

impl ImportRentalFleetInput {
    pub const fn new(bicycles: Vec<ImportedBicycle>) -> Self {
        Self { bicycles }
    }

    pub(super) fn into_bicycles(self) -> Vec<ImportedBicycle> {
        self.bicycles
    }
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-fleet-imported", label = "Rental fleet imported")]
pub struct RentalFleetImported {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(value_object)]
    pub bicycles: Vec<ImportedBicycle>,
}

impl Apply<RentalFleetImported> for RentalFleet {
    fn apply(&mut self, event: &RentalFleetImported) {
        *self = Self::new(
            self.fleet_id().clone(),
            event
                .bicycles
                .iter()
                .map(|bicycle| {
                    Bicycle::new(
                        bicycle.bicycle_id.clone(),
                        bicycle.status,
                        bicycle.condition,
                    )
                })
                .collect(),
        );
    }
}
