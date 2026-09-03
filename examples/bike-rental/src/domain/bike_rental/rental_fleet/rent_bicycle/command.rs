use rostfrei::Command;

use crate::domain::rental_fleet::BicycleId;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "rent-bicycle", label = "Rent bicycle")]
pub struct RentBicycle {
    pub bicycle_id: BicycleId,
}
