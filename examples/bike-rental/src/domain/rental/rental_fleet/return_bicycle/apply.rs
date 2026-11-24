use rostfrei::Apply;

use super::BicycleReturned;
use crate::domain::rental_fleet::RentalFleet;

impl Apply<BicycleReturned> for RentalFleet {
    fn apply(&mut self, event: &BicycleReturned) {
        for reservation in &mut self.reservations {
            if let Some(collection) = reservation.collection.as_mut()
                && collection.collected
                && collection.bicycle_id == event.bicycle_id
            {
                collection.returned = true;
            }
        }
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == &event.bicycle_id)
        {
            bicycle.apply_returned();
        }
    }
}
