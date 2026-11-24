use rostfrei::{AggregateInstance, LifecycleState};

use super::{BicycleCannotBeRetired, BicycleRetired, RetireBicycleAction};
use crate::domain::rental_fleet::{BicycleId, BicycleRentalTransition, RentalFleetAggregate};

impl RetireBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn retire_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleCannotBeRetired> {
        let event = {
            let root = self.state();
            let bicycle = root
                .bicycles
                .iter()
                .find(|bicycle| bicycle.bicycle_id() == &input)
                .ok_or_else(|| BicycleCannotBeRetired {
                    bicycle_id: input.clone(),
                })?;
            let _change = bicycle
                .status()
                .evaluate(&BicycleRentalTransition::Retire)
                .map_err(|_| BicycleCannotBeRetired {
                    bicycle_id: input.clone(),
                })?;
            BicycleRetired {
                fleet_id: root.fleet_id.clone(),
                bicycle_id: input,
            }
        };

        self.raise(event);
        Ok(())
    }
}
