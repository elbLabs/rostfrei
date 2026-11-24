use super::SecurityDepositRefunded;
use crate::domain::rental_payment::RentalPayment;
use rostfrei::Apply;

impl Apply<SecurityDepositRefunded> for RentalPayment {
    fn apply(&mut self, event: &SecurityDepositRefunded) {
        self.refund_reference = Some(event.refund_reference.clone());
    }
}
