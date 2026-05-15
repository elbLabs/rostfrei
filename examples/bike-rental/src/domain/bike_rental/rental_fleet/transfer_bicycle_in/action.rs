use rostfrei::domain_action;

use crate::domain::rental_fleet::{BicycleCondition, BicycleId, FleetId};

#[domain_action(id = "transfer-bicycle-in", label = "Transfer bicycle in")]
pub trait TransferBicycleInAction {
    fn transfer_bicycle_in(
        &mut self,
        bicycle_id: BicycleId,
        from_fleet_id: FleetId,
        condition: BicycleCondition,
    );
}
