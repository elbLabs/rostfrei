use crate::domain::{Finance, rental_payment::PaymentId};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "refund-eligible-rental-deposit", label = "Refund eligible rental deposit", context = Finance)]
pub struct RefundEligibleRentalDeposit {
    pub payment_id: PaymentId,
}
