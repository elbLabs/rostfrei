use super::{BicycleCollectionCancelled, CancelCollectionAction};
use crate::domain::{
    rental_fleet::{BicycleId, RentalFleetAggregate, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::AggregateInstance;

impl CancelCollectionAction for AggregateInstance<RentalFleetAggregate> {
    fn cancel_collection(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
    ) {
        self.raise(BicycleCollectionCancelled {
            fleet_id: self.state().fleet_id().clone(),
            reservation_id,
            bicycle_id,
            payment_id,
        });
    }
}
