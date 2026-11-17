use crate::domain::{BikeRental, rental_payment::PaymentId};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "process-rental-payment", label = "Process rental payment", context = BikeRental)]
pub struct ProcessRentalPayment {
    pub payment_id: PaymentId,
}
