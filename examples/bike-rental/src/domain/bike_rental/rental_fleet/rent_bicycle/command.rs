use rostfrei::Command;

use crate::domain::{
    BikeRental,
    rental_fleet::{BicycleId, FleetId},
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    context = BikeRental,
    schema_version = 2
)]
pub struct RentBicycle {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
}
