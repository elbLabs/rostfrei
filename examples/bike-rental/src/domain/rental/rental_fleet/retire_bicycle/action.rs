use rostfrei::domain_action;

use super::BicycleCannotBeRetired;
use crate::domain::rental_fleet::BicycleId;

#[domain_action(id = "retire-bicycle", label = "Retire bicycle")]
pub trait RetireBicycleAction {
    fn retire_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleCannotBeRetired>;
}
