use super::{RecordPaymentIdentityConflictAction, RentalPaymentIdentityConflictRecorded};
use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::RentalPaymentAggregate,
};
use rostfrei::AggregateInstance;

impl RecordPaymentIdentityConflictAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_payment_identity_conflict(
        &mut self,
        fleet_id: FleetId,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
    ) {
        if !self.state().conflicting_fleets.contains(&fleet_id) {
            self.raise(RentalPaymentIdentityConflictRecorded {
                payment_id: self.state().payment_id().clone(),
                fleet_id,
                reservation_id,
                bicycle_id,
            });
        }
    }
}
