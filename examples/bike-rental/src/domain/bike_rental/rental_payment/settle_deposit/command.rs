use crate::domain::{BikeRental, rental_payment::PaymentId};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "settle-rental-deposit", label = "Settle rental deposit", context = BikeRental)]
pub struct SettleRentalDeposit {
    pub payment_id: PaymentId,
}
