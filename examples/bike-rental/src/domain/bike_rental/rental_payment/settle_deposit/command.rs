use crate::domain::{BikeRental, rental_payment::PaymentId};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "refund-eligible-rental-deposit", label = "Refund eligible rental deposit", context = BikeRental)]
pub struct RefundEligibleRentalDeposit {
    pub payment_id: PaymentId,
}
