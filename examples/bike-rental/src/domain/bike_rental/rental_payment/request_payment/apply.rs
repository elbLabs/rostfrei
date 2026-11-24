use super::RentalPaymentRequested;
use crate::domain::rental_payment::RentalPayment;
use rostfrei::Apply;

impl Apply<RentalPaymentRequested> for RentalPayment {
    fn apply(&mut self, event: &RentalPaymentRequested) {
        self.details = Some(event.details.clone());
    }
}
