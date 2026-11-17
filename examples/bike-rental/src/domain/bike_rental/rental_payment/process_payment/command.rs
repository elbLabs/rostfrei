use crate::domain::{BikeRental, rental_payment::PaymentId};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "collect-rental-payment", label = "Collect rental payment", context = BikeRental)]
pub struct CollectRentalPayment {
    pub payment_id: PaymentId,
}
