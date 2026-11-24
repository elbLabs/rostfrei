use crate::domain::rental_payment::{PaymentOutcome, PaymentRejected};
use rostfrei::domain_action;

#[domain_action(id = "record-payment-result", label = "Record payment result")]
pub trait RecordPaymentResultAction {
    fn record_payment_result(
        &mut self,
        outcome: PaymentOutcome,
        provider_reference: String,
    ) -> Result<(), PaymentRejected>;
}
