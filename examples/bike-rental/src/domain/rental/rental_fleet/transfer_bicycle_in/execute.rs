use rostfrei::AggregateInstance;

use super::{BicycleTransferredIn, TransferBicycleInAction};
use crate::domain::rental_fleet::{
    BicycleCategory, BicycleCondition, BicycleId, FleetId, RentalFleetAggregate,
};

impl TransferBicycleInAction for AggregateInstance<RentalFleetAggregate> {
    fn transfer_bicycle_in(
        &mut self,
        bicycle_id: BicycleId,
        from_fleet_id: FleetId,
        condition: BicycleCondition,
        category: Option<BicycleCategory>,
    ) {
        self.raise(BicycleTransferredIn {
            fleet_id: self.state().fleet_id().clone(),
            bicycle_id,
            from_fleet_id,
            condition,
            category,
        });
    }
}
