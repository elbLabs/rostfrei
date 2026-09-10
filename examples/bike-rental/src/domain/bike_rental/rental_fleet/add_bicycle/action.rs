use rostfrei::domain_action;

use super::BicycleAlreadyInFleet;
use crate::domain::rental_fleet::{BicycleCondition, BicycleId};

#[domain_action(id = "add-bicycle", label = "Add bicycle")]
pub trait AddBicycleAction {
    fn add_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        condition: BicycleCondition,
    ) -> Result<(), BicycleAlreadyInFleet>;
}
