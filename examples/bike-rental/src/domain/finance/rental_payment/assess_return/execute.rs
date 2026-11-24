use super::{RecordRentalReturnEligibilityAction, RentalReturnEligibilityRecorded};
use crate::domain::{
    rental_fleet::{BicycleId, FleetId, ReservationId},
    rental_payment::{PaymentOutcome, PaymentRejected, RentalPaymentAggregate},
};
use rostfrei::AggregateInstance;

impl RecordRentalReturnEligibilityAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_rental_return_eligibility(
        &mut self,
        fleet_id: &FleetId,
        reservation_id: &ReservationId,
        bicycle_id: &BicycleId,
        eligible: bool,
    ) -> Result<(), PaymentRejected> {
        let state = self.state();
        let reject = |reason: &str| PaymentRejected {
            payment_id: state.payment_id().clone(),
            reason: reason.to_owned(),
        };
        if !state.details().is_some_and(|details| {
            &details.fleet_id == fleet_id
                && &details.reservation_id == reservation_id
                && &details.bicycle_id == bicycle_id
        }) {
            return Err(reject("payment-identity-conflict"));
        }
        if state.outcome() != Some(PaymentOutcome::Approved) {
            return Err(reject("payment-not-approved"));
        }
        if let Some(existing) = state.return_eligible() {
            return if existing == eligible {
                Ok(())
            } else {
                Err(reject("return-eligibility-conflict"))
            };
        }
        self.raise(RentalReturnEligibilityRecorded {
            payment_id: state.payment_id().clone(),
            eligible,
        });
        Ok(())
    }
}
