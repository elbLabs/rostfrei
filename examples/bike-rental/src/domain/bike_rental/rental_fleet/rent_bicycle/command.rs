use rostfrei::Command;

use super::BicycleUnavailable;
use crate::domain::rental_fleet::{BicycleId, RentalFleetAggregate};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleUnavailable,
    json,
    runtime
)]
pub struct RentBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}
