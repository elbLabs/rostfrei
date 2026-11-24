use rostfrei::domain_action;

use super::BicycleNotRented;
use crate::domain::rental_fleet::BicycleId;

#[domain_action(id = "return-bicycle", label = "Return bicycle")]
pub trait ReturnBicycleAction {
    fn return_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleNotRented>;
}
