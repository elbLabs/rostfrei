use rostfrei::Command;

use crate::domain::rental_fleet::{BicycleId, FleetId};

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "transfer-bicycle", label = "Transfer bicycle")]
pub struct TransferBicycle {
    pub bicycle_id: BicycleId,
    pub to_fleet_id: FleetId,
}
