use rostfrei::Apply;

use super::BicycleRented;
use crate::domain::bike_rental::rental_fleet::{
    RentalFleet, bicycle::mark_rented::MarkRentedAction,
};

impl Apply<BicycleRented> for RentalFleet {
    fn apply(&mut self, event: &BicycleRented) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.mark_rented();
        }
    }
}
