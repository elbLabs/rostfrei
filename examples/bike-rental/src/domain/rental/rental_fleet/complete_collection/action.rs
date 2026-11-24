use crate::domain::{
    rental_fleet::{BicycleId, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::domain_action;

#[domain_action(
    id = "complete-bicycle-collection",
    label = "Complete bicycle collection"
)]
pub trait CompleteCollectionAction {
    fn complete_collection(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
    );
}
