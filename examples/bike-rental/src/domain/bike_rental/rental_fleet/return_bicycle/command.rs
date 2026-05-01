use rostfrei::Command;

use crate::domain::rental_fleet::BicycleId;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "return-bicycle", label = "Return bicycle")]
pub struct ReturnBicycle {
    pub bicycle_id: BicycleId,
}
