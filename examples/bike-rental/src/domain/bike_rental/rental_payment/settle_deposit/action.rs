use crate::domain::rental_payment::PaymentRejected;
use rostfrei::domain_action;

#[domain_action(id = "record-deposit-refund", label = "Record deposit refund")]
pub trait RecordDepositRefundAction {
    fn record_deposit_refund(&mut self, refund_reference: String) -> Result<(), PaymentRejected>;
}
