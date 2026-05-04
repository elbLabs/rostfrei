use rostfrei::Apply;

use super::BicycleRented;
use crate::domain::bike_rental::rental_fleet::{BicycleRentalTransition, RentalFleet};

impl Apply<BicycleRented> for RentalFleet {
    fn apply(&mut self, event: &BicycleRented) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.apply_transition(BicycleRentalTransition::Rent);
        }
    }
}
