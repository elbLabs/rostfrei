use super::RentalPaymentIdentityConflictRecorded;
use crate::domain::rental_payment::RentalPayment;
use rostfrei::Apply;

impl Apply<RentalPaymentIdentityConflictRecorded> for RentalPayment {
    fn apply(&mut self, event: &RentalPaymentIdentityConflictRecorded) {
        self.conflicting_fleets.push(event.fleet_id.clone());
    }
}
