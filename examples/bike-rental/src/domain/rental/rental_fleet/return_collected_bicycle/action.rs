use crate::domain::rental_fleet::{BicycleId, BicycleNotRented, ReturnTime};
use rostfrei::domain_action;

#[domain_action(id = "return-collected-bicycle", label = "Return collected bicycle")]
pub trait ReturnCollectedBicycleAction {
    fn return_collected_bicycle(
        &mut self,
        bicycle_id: BicycleId,
        returned_at: Option<ReturnTime>,
        damage_report: Option<String>,
    ) -> Result<(), BicycleNotRented>;
}
