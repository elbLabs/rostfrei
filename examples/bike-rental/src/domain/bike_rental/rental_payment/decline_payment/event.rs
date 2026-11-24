use crate::domain::{rental_payment::PaymentId, rental_tariff::RentalQuote};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-payment-declined", label = "Rental payment declined")]
pub struct RentalPaymentDeclined {
    pub payment_id: PaymentId,
    pub provider_reference: String,
    pub quote: RentalQuote,
}
