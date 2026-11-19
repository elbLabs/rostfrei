use crate::domain::rental_tariff::RentalQuote;
use rostfrei::domain_action;

#[domain_action(id = "record-payment-decline", label = "Record payment decline")]
pub trait RecordPaymentDeclineAction {
    fn record_payment_decline(&mut self, provider_reference: String, quote: RentalQuote);
}
