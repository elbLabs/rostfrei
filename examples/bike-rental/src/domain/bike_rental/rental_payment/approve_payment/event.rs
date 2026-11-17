use crate::domain::{rental_payment::PaymentId, rental_tariff::RentalQuote};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-payment-approved", label = "Rental payment approved")]
pub struct RentalPaymentApproved {
    pub payment_id: PaymentId,
    pub provider_reference: String,
    pub quote: RentalQuote,
}
