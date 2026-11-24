use super::ReservedBicycleCollected;
use crate::domain::rental_fleet::RentalFleet;
use rostfrei::Apply;

impl Apply<ReservedBicycleCollected> for RentalFleet {
    fn apply(&mut self, event: &ReservedBicycleCollected) {
        if let Some(reservation) = self
            .reservations
            .iter_mut()
            .find(|r| r.reservation_id() == &event.reservation_id)
            && let Some(collection) = reservation.collection.as_mut()
        {
            collection.collected = true;
        }
    }
}
