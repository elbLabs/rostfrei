use super::{BicycleCollectionRequested, CollectReservedBicycleAction, CollectionRejected};
use crate::domain::{
    rental_fleet::{
        BicycleCondition, BicycleId, BicycleStatus, RentalFleetAggregate, ReservationId,
    },
    rental_payment::{CollectionPayment, PaymentId},
};
use rostfrei::AggregateInstance;

impl CollectReservedBicycleAction for AggregateInstance<RentalFleetAggregate> {
    fn collect_reserved_bicycle(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
        payment_method: String,
    ) -> Result<CollectionPayment, CollectionRejected> {
        let reject = |reason: &str| CollectionRejected {
            reservation_id: reservation_id.clone(),
            reason: reason.to_owned(),
        };
        let root = self.state();
        let reservation = root
            .reservations()
            .iter()
            .find(|r| r.reservation_id() == &reservation_id)
            .ok_or_else(|| reject("reservation-not-found"))?;
        if reservation.collection().is_some() {
            return Err(reject("collection-already-started"));
        }
        let bike = root
            .bicycles()
            .iter()
            .find(|b| b.bicycle_id() == &bicycle_id)
            .ok_or_else(|| reject("bicycle-not-found"))?;
        if bike.category() != Some(reservation.category()) {
            return Err(reject("wrong-category"));
        }
        if bike.status() != BicycleStatus::Available
            || bike.condition() != BicycleCondition::Serviceable
        {
            return Err(reject("bicycle-unavailable"));
        }
        let quote = reservation
            .quote()
            .cloned()
            .filter(|quote| quote.total_due_minor().is_some())
            .ok_or_else(|| reject("quote-unavailable"))?;
        if payment_method.trim().is_empty() {
            return Err(reject("missing-payment-method"));
        }
        let details = CollectionPayment {
            fleet_id: root.fleet_id().clone(),
            reservation_id: reservation_id.clone(),
            bicycle_id: bicycle_id.clone(),
            customer_id: reservation.customer_id().to_owned(),
            quote,
            payment_method,
        };
        self.raise(BicycleCollectionRequested {
            fleet_id: details.fleet_id.clone(),
            reservation_id,
            bicycle_id,
            payment_id,
        });
        Ok(details)
    }
}
