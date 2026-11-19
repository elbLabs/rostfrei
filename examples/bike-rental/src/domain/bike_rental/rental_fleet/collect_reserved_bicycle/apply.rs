use super::BicycleCollectionRequested;
use crate::domain::rental_fleet::{CollectionAssignment, RentalFleet};
use rostfrei::Apply;

impl Apply<BicycleCollectionRequested> for RentalFleet {
    fn apply(&mut self, event: &BicycleCollectionRequested) {
        if let Some(bike) = self
            .bicycles
            .iter_mut()
            .find(|bike| bike.bicycle_id() == &event.bicycle_id)
        {
            bike.apply_collection_requested();
        }
        if let Some(reservation) = self
            .reservations
            .iter_mut()
            .find(|reservation| reservation.reservation_id() == &event.reservation_id)
        {
            reservation.collection = Some(CollectionAssignment {
                bicycle_id: event.bicycle_id.clone(),
                payment_id: event.payment_id.clone(),
                collected: false,
                returned: false,
                return_details: None,
            });
        }
    }
}
