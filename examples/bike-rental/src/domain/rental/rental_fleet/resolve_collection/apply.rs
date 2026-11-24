use super::BicycleCollectionResolved;
use crate::domain::{rental_fleet::RentalFleet, rental_payment::PaymentOutcome};
use rostfrei::Apply;

impl Apply<BicycleCollectionResolved> for RentalFleet {
    fn apply(&mut self, event: &BicycleCollectionResolved) {
        if let Some(reservation) = self
            .reservations
            .iter_mut()
            .find(|r| r.reservation_id() == &event.reservation_id)
        {
            match event.outcome {
                PaymentOutcome::Approved => {
                    if let Some(collection) = reservation.collection.as_mut() {
                        collection.collected = true;
                    }
                }
                PaymentOutcome::Declined => {
                    reservation.collection = None;
                }
            }
        }
        if event.outcome == PaymentOutcome::Declined
            && let Some(bike) = self
                .bicycles
                .iter_mut()
                .find(|b| b.bicycle_id() == &event.bicycle_id)
        {
            bike.apply_returned();
        }
    }
}
