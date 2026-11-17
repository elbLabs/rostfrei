use super::{BicycleCollectionResolved, ResolveCollectionAction};
use crate::domain::{
    rental_fleet::{BicycleRented, CollectionRejected, RentalFleetAggregate, ReservationId},
    rental_payment::{PaymentId, PaymentOutcome},
};
use rostfrei::AggregateInstance;

impl ResolveCollectionAction for AggregateInstance<RentalFleetAggregate> {
    fn resolve_collection(
        &mut self,
        reservation_id: ReservationId,
        payment_id: PaymentId,
        outcome: PaymentOutcome,
    ) -> Result<(), CollectionRejected> {
        let collection = self
            .state()
            .pending_collection(&reservation_id, &payment_id)
            .ok_or_else(|| CollectionRejected {
                reservation_id: reservation_id.clone(),
                reason: "collection-not-pending".to_owned(),
            })?;
        let event = BicycleCollectionResolved {
            fleet_id: self.state().fleet_id().clone(),
            reservation_id,
            bicycle_id: collection.bicycle_id.clone(),
            payment_id,
            outcome,
        };
        if outcome == PaymentOutcome::Approved {
            self.raise(BicycleRented {
                fleet_id: event.fleet_id.clone(),
                bicycle_id: event.bicycle_id.clone(),
            });
        }
        self.raise(event);
        Ok(())
    }
}
