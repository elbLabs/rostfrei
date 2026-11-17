use rostfrei::domain_action;

use crate::domain::rental_fleet::{BicycleId, BicycleNotRented};

#[domain_action(id = "return-damaged-bicycle", label = "Return damaged bicycle")]
pub trait ReturnDamagedBicycleAction {
    fn return_damaged_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        damage_report: String,
    ) -> Result<(), BicycleNotRented>;
}
