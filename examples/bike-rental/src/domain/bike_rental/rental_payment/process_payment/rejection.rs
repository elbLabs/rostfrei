use crate::domain::rental_payment::PaymentId;
use rostfrei::DomainError;

#[derive(DomainError, Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[domain(
    id = "payment-rejected",
    label = "Payment rejected",
    code = "PAYMENT_REJECTED",
    message = "The rental payment cannot be processed."
)]
pub struct PaymentRejected {
    pub payment_id: PaymentId,
    pub reason: String,
}
