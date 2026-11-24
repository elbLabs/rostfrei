use crate::domain::{Finance, rental_payment::PaymentId};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "collect-rental-payment", label = "Collect rental payment", context = Finance)]
pub struct CollectRentalPayment {
    pub payment_id: PaymentId,
}
