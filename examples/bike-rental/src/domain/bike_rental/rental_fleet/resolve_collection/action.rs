use crate::domain::{
    rental_fleet::{CollectionRejected, ReservationId},
    rental_payment::{PaymentId, PaymentOutcome},
};
use rostfrei::domain_action;

#[domain_action(id = "resolve-collection", label = "Resolve collection")]
pub trait ResolveCollectionAction {
    fn resolve_collection(
        &mut self,
        reservation_id: ReservationId,
        payment_id: PaymentId,
        outcome: PaymentOutcome,
    ) -> Result<(), CollectionRejected>;
}
