use super::{RecordPaymentApprovalAction, RentalPaymentApproved};
use crate::domain::{rental_payment::RentalPaymentAggregate, rental_tariff::RentalQuote};
use rostfrei::AggregateInstance;

impl RecordPaymentApprovalAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_payment_approval(&mut self, provider_reference: String, quote: RentalQuote) {
        self.raise(RentalPaymentApproved {
            payment_id: self.state().payment_id().clone(),
            provider_reference,
            quote,
        });
    }
}
