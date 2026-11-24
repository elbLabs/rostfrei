use crate::domain::{
    Finance,
    rental_payment::{CollectionPayment, PaymentId},
};
use rostfrei::Command;

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(id = "record-rental-payment-request", label = "Record rental payment request", context = Finance)]
pub struct RecordRentalPaymentRequest {
    pub payment_id: PaymentId,
    pub details: CollectionPayment,
}
