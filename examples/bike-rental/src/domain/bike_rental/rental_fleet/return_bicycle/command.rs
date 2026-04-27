use rostfrei::Command;

use super::BicycleNotRented;
use crate::domain::rental_fleet::{BicycleId, RentalFleetAggregate};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "return-bicycle",
    label = "Return bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleNotRented,
    json,
    runtime
)]
pub struct ReturnBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}
