use super::ResolveCollectionAction;
use crate::domain::{
    rental_fleet::{
        CancelCollectionAction as _, CollectionRejected, CompleteCollectionAction as _,
        RentalFleetAggregate, ReservationId,
    },
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
        let bicycle_id = collection.bicycle_id.clone();
        if outcome == PaymentOutcome::Approved {
            self.complete_collection(reservation_id, bicycle_id, payment_id);
        } else {
            self.cancel_collection(reservation_id, bicycle_id, payment_id);
        }
        Ok(())
    }
}
