use super::ResolveCollectionAction;
use crate::domain::{
    rental_fleet::{
        BicycleId, CancelCollectionAction as _, CollectionRejected, CompleteCollectionAction as _,
        RentalFleetAggregate, ReservationId,
    },
    rental_payment::{PaymentId, PaymentOutcome},
};
use rostfrei::AggregateInstance;

impl ResolveCollectionAction for AggregateInstance<RentalFleetAggregate> {
    fn resolve_collection(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
        outcome: PaymentOutcome,
    ) -> Result<(), CollectionRejected> {
        let root = self.state();
        if !root
            .collection_requests
            .get(payment_id.as_str())
            .is_some_and(|request| {
                request.reservation_id == reservation_id && request.bicycle_id == bicycle_id
            })
        {
            return Err(CollectionRejected {
                reservation_id,
                reason: "collection-identity-conflict".to_owned(),
            });
        }
        if root
            .pending_collection(&reservation_id, &payment_id)
            .is_none()
        {
            // The exact attempt is known but is no longer pending. Never touch a later hold.
            return Ok(());
        }
        if outcome == PaymentOutcome::Approved {
            self.complete_collection(reservation_id, bicycle_id, payment_id);
        } else {
            self.cancel_collection(reservation_id, bicycle_id, payment_id);
        }
        Ok(())
    }
}
