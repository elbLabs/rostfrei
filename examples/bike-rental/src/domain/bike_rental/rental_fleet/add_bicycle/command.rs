use rostfrei::Command;

use crate::domain::{
    BikeRental,
    rental_fleet::{BicycleCategory, BicycleCondition, BicycleId, FleetId},
};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "register-bicycle",
    label = "Register bicycle",
    context = BikeRental,
    schema_version = 2
)]
pub struct RegisterBicycle {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
    pub condition: BicycleCondition,
    pub category: Option<BicycleCategory>,
}
