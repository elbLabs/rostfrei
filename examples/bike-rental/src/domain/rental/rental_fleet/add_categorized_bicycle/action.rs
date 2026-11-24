use rostfrei::domain_action;

use crate::domain::rental_fleet::{
    BicycleAlreadyInFleet, BicycleCategory, BicycleCondition, BicycleId,
};

#[domain_action(id = "add-categorized-bicycle", label = "Add categorized bicycle")]
pub trait AddCategorizedBicycleAction {
    fn add_categorized_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        condition: BicycleCondition,
        category: BicycleCategory,
    ) -> Result<(), BicycleAlreadyInFleet>;
}
