use super::CollectionRejected;
use crate::domain::{
    rental_fleet::{BicycleId, ReservationId},
    rental_payment::{CollectionPayment, PaymentId},
};
use rostfrei::domain_action;

#[domain_action(id = "collect-reserved-bicycle", label = "Collect reserved bicycle")]
pub trait CollectReservedBicycleAction {
    fn collect_reserved_bicycle(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
        payment_method: String,
    ) -> Result<CollectionPayment, CollectionRejected>;
}
