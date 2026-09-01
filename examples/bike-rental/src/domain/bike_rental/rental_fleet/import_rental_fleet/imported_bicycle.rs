use rostfrei::ValueObject;
use serde::{Deserialize, Serialize};

use crate::domain::rental_fleet::{
    BicycleCondition, BicycleId, BicycleStatus, RentalFleetAggregate,
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
