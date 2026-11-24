use super::RentalPaymentApproved;
use crate::domain::rental_payment::{PaymentOutcome, RentalPayment};
use rostfrei::Apply;

impl Apply<RentalPaymentApproved> for RentalPayment {
    fn apply(&mut self, event: &RentalPaymentApproved) {
        self.outcome = Some(PaymentOutcome::Approved);
        self.provider_reference = Some(event.provider_reference.clone());
    }
}
