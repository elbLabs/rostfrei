use rostfrei::domain_action;

use crate::domain::rental_fleet::{BicycleCondition, BicycleId, FleetId};

#[domain_action(id = "transfer-bicycle-out", label = "Transfer bicycle out")]
pub trait TransferBicycleOutAction {
    fn transfer_bicycle_out(
        &mut self,
        bicycle_id: BicycleId,
        to_fleet_id: FleetId,
        condition: BicycleCondition,
    );
}
