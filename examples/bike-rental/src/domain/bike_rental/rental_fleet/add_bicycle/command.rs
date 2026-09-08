use rostfrei::Command;

use crate::domain::{BikeRental, rental_fleet::FleetId};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "add-bicycle",
    label = "Add bicycle",
    context = BikeRental,
    schema_version = 2
)]
pub struct AddBicycle {
    pub fleet_id: FleetId,
}
