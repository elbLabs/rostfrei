use rostfrei::Command;

use crate::domain::{
    BikeRental,
    rental_fleet::{BicycleId, FleetId},
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "transfer-bicycle",
    label = "Transfer bicycle",
    context = BikeRental,
    schema_version = 2
)]
pub struct TransferBicycle {
    pub from_fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub to_fleet_id: FleetId,
}
