use crate::domain::rental_payment::{CollectionPayment, PaymentId};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-payment-requested", label = "Rental payment requested")]
pub struct RentalPaymentRequested {
    pub payment_id: PaymentId,
    pub details: CollectionPayment,
}
