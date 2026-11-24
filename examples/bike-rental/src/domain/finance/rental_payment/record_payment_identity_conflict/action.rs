use crate::domain::rental_fleet::{BicycleId, FleetId, ReservationId};
use rostfrei::domain_action;

#[domain_action(
    id = "record-payment-identity-conflict",
    label = "Record payment identity conflict"
)]
pub trait RecordPaymentIdentityConflictAction {
    fn record_payment_identity_conflict(
        &mut self,
        fleet_id: FleetId,
        reservation_id: ReservationId,
        bicycle_id: BicycleId,
    );
}
