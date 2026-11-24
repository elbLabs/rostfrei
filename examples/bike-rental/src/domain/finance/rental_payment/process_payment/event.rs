use crate::domain::{
    rental_payment::{PaymentId, PaymentOutcome},
    rental_tariff::RentalQuote,
};
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

// Retained for replay; new payment decisions write explicit approval/decline events.
#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-payment-processed", label = "Rental payment processed")]
pub struct RentalPaymentProcessed {
    pub payment_id: PaymentId,
    pub outcome: PaymentOutcome,
    pub provider_reference: String,
    pub quote: RentalQuote,
}
