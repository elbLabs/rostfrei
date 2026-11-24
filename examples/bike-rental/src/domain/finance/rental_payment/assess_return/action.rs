use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::PaymentRejected,
};
use rostfrei::domain_action;

#[domain_action(
    id = "record-rental-return-eligibility",
    label = "Record rental return eligibility"
)]
pub trait RecordRentalReturnEligibilityAction {
    fn record_rental_return_eligibility(
        &mut self,
        fleet_id: &FleetId,
        reservation_id: &ReservationId,
        bicycle_id: &BicycleId,
        eligible: bool,
    ) -> Result<(), PaymentRejected>;
}
