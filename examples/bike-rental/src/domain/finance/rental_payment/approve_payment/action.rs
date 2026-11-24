use crate::domain::rental_payment::CollectionPayment;
use rostfrei::domain_action;

#[domain_action(id = "record-payment-approval", label = "Record payment approval")]
pub trait RecordPaymentApprovalAction {
    fn record_payment_approval(&mut self, provider_reference: String, details: CollectionPayment);
}
