use rostfrei::domain_actions;

use super::BicycleNotRented;
use crate::domain::rental_fleet::BicycleId;

#[domain_actions(aggregate(instance = ReturnBicycleActions))]
pub trait ReturnBicycleActionContract {
    #[action(id = "return-bicycle", label = "Return bicycle")]
    fn return_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleNotRented>;
}
