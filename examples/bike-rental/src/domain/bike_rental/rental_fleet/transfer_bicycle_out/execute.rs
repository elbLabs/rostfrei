use rostfrei::AggregateInstance;

use super::{BicycleTransferredOut, TransferBicycleOutAction};
use crate::domain::rental_fleet::{BicycleCondition, BicycleId, FleetId, RentalFleetAggregate};

impl TransferBicycleOutAction for AggregateInstance<RentalFleetAggregate> {
    fn transfer_bicycle_out(
        &mut self,
        bicycle_id: BicycleId,
        to_fleet_id: FleetId,
        condition: BicycleCondition,
    ) {
        self.raise(BicycleTransferredOut {
            fleet_id: self.state().fleet_id().clone(),
            bicycle_id,
            to_fleet_id,
            condition,
        });
    }
}
