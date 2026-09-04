use rostfrei::Apply;

use super::BicycleReturned;
use crate::domain::rental_fleet::{BicycleRentalTransition, RentalFleet};

impl Apply<BicycleReturned> for RentalFleet {
    fn apply(&mut self, event: &BicycleReturned) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.apply_transition(BicycleRentalTransition::Return);
        }
    }
}
