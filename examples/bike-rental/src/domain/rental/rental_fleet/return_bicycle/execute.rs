use rostfrei::{AggregateInstance, LifecycleState};

use super::{BicycleNotRented, BicycleReturned, ReturnBicycleAction};
use crate::domain::rental_fleet::{BicycleId, BicycleRentalTransition, RentalFleetAggregate};

impl ReturnBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn return_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleNotRented> {
        let root = self.state();
        let rented = root.bicycles.iter().any(|bicycle| {
            bicycle.bicycle_id() == &input
                && bicycle
                    .status()
                    .evaluate(&BicycleRentalTransition::Return)
                    .is_ok()
        });
        if !rented {
            return Err(BicycleNotRented { bicycle_id: input });
        }
        let fleet_id = root.fleet_id.clone();
        self.raise(BicycleReturned {
            fleet_id,
            bicycle_id: input,
        });
        Ok(())
    }
}
