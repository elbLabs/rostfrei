use crate::domain::rental_payment::{CollectionPayment, PaymentRejected};
use rostfrei::domain_action;

#[domain_action(id = "request-rental-payment", label = "Request rental payment")]
pub trait RequestRentalPaymentAction {
    fn request_rental_payment(&mut self, details: CollectionPayment)
    -> Result<(), PaymentRejected>;
}
