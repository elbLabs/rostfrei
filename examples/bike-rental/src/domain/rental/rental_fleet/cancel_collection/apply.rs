use super::BicycleCollectionCancelled;
use crate::domain::rental_fleet::RentalFleet;
use rostfrei::Apply;

impl Apply<BicycleCollectionCancelled> for RentalFleet {
    fn apply(&mut self, event: &BicycleCollectionCancelled) {
        if let Some(reservation) = self
            .reservations
            .iter_mut()
            .find(|r| r.reservation_id() == &event.reservation_id)
        {
            reservation.collection = None;
        }
        if let Some(bike) = self
            .bicycles
            .iter_mut()
            .find(|b| b.bicycle_id() == &event.bicycle_id)
        {
            bike.apply_returned();
        }
    }
}
