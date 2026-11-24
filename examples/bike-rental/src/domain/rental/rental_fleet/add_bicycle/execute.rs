use rostfrei::AggregateInstance;

use super::{AddBicycleAction, BicycleAdded, BicycleAlreadyInFleet};
use crate::domain::rental_fleet::{BicycleCondition, BicycleId, RentalFleetAggregate};

impl AddBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn add_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        condition: BicycleCondition,
    ) -> Result<(), BicycleAlreadyInFleet> {
        if self
            .state()
            .bicycles
            .iter()
            .any(|bicycle| bicycle.bicycle_id() == &bicycle_id)
        {
            return Err(BicycleAlreadyInFleet { bicycle_id });
        }

        let fleet_id = self.state().fleet_id.clone();
        self.raise(BicycleAdded {
            fleet_id,
            bicycle_id,
            condition,
        });
        Ok(())
    }
}
