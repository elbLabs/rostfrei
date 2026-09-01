use rostfrei::Command;

use crate::domain::rental_fleet::RentalFleetAggregate;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "add-bicycle",
    label = "Add bicycle",
    owner = RentalFleetAggregate,
    json,
    runtime
)]
pub struct AddBicycle;
