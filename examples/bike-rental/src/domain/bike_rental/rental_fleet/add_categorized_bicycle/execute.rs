use rostfrei::AggregateInstance;

use super::{AddCategorizedBicycleAction, BicycleCategorized};
use crate::domain::rental_fleet::{
    AddBicycleAction as _, BicycleAlreadyInFleet, BicycleCategory, BicycleCondition, BicycleId,
    RentalFleetAggregate,
};

impl AddCategorizedBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn add_categorized_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        condition: BicycleCondition,
        category: BicycleCategory,
    ) -> Result<(), BicycleAlreadyInFleet> {
        self.add_bicycle(bicycle_id.clone(), condition)?;
        self.raise(BicycleCategorized {
            fleet_id: self.state().fleet_id().clone(),
            bicycle_id,
            category,
        });
        Ok(())
    }
}
