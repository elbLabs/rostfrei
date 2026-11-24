use super::{CompleteCollectionAction, ReservedBicycleCollected};
use crate::domain::{
    rental_fleet::{BicycleId, BicycleRented, RentalFleetAggregate, ReservationId},
    rental_payment::PaymentId,
};
use rostfrei::AggregateInstance;

impl CompleteCollectionAction for AggregateInstance<RentalFleetAggregate> {
    fn complete_collection(
        &mut self,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
        payment_id: PaymentId,
    ) {
        let fleet_id = self.state().fleet_id().clone();
        self.raise(BicycleRented {
            fleet_id: fleet_id.clone(),
            bicycle_id: bicycle_id.clone(),
        });
        self.raise(ReservedBicycleCollected {
            fleet_id,
            reservation_id,
            bicycle_id,
            payment_id,
        });
    }
}
