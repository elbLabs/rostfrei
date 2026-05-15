use rostfrei::Command;

use crate::domain::{
    BikeRental,
    rental_fleet::{BicycleId, FleetId},
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "return-bicycle",
    label = "Return bicycle",
    context = BikeRental,
    schema_version = 2
)]
pub struct ReturnBicycle {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
}
