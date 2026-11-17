use super::ReservedBicycleReturned;
use crate::domain::rental_fleet::RentalFleet;
use rostfrei::Apply;

impl Apply<ReservedBicycleReturned> for RentalFleet {
    fn apply(&mut self, event: &ReservedBicycleReturned) {
        if let Some(reservation) = self
            .reservations
            .iter_mut()
            .find(|reservation| reservation.reservation_id() == &event.reservation_id)
            && let Some(collection) = reservation.collection.as_mut()
            && collection.payment_id == event.payment_id
            && collection.bicycle_id == event.bicycle_id
        {
            collection.returned = true;
            collection.return_details = Some(event.details.clone());
        }
    }
}
