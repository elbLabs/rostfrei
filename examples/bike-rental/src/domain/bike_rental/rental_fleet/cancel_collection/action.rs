use crate::domain::{
    rental_fleet::{BicycleId, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::domain_action;

#[domain_action(id = "cancel-bicycle-collection", label = "Cancel bicycle collection")]
pub trait CancelCollectionAction {
    fn cancel_collection(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
    );
}
