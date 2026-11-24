use super::RentalPaymentDeclined;
use crate::domain::rental_payment::{PaymentOutcome, RentalPayment};
use rostfrei::Apply;

impl Apply<RentalPaymentDeclined> for RentalPayment {
    fn apply(&mut self, event: &RentalPaymentDeclined) {
        self.outcome = Some(PaymentOutcome::Declined);
        self.provider_reference = Some(event.provider_reference.clone());
    }
}
