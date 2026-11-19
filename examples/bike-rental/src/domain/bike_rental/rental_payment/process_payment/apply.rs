use super::RentalPaymentProcessed;
use crate::domain::rental_payment::RentalPayment;
use rostfrei::Apply;

impl Apply<RentalPaymentProcessed> for RentalPayment {
    fn apply(&mut self, event: &RentalPaymentProcessed) {
        self.outcome = Some(event.outcome);
        self.provider_reference = Some(event.provider_reference.clone());
    }
}
