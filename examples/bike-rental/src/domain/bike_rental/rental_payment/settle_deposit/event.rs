use crate::domain::rental_payment::PaymentId;
use rostfrei::DomainEvent;
use serde::{Deserialize, Serialize};

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "security-deposit-refunded", label = "Security deposit refunded")]
pub struct SecurityDepositRefunded {
    pub payment_id: PaymentId,
    pub currency: String,
    pub amount_minor: u64,
    pub charge_reference: String,
    pub refund_reference: String,
}
