use rostfrei::Apply;

use super::BicycleReturned;
use crate::domain::{
    bike_rental::rental_fleet::bicycle::mark_available::MarkAvailableAction as _,
    rental_fleet::RentalFleet,
};

impl Apply<BicycleReturned> for RentalFleet {
    fn apply(&mut self, event: &BicycleReturned) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.mark_available();
        }
    }
}
