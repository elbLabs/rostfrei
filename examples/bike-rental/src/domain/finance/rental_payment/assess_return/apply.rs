use super::RentalReturnEligibilityRecorded;
use crate::domain::rental_payment::RentalPayment;
use rostfrei::Apply;

impl Apply<RentalReturnEligibilityRecorded> for RentalPayment {
    fn apply(&mut self, event: &RentalReturnEligibilityRecorded) {
        self.return_eligible = Some(event.eligible);
    }
}
