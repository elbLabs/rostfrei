use super::{RecordPaymentDeclineAction, RentalPaymentDeclined};
use crate::domain::{rental_payment::RentalPaymentAggregate, rental_tariff::RentalQuote};
use rostfrei::AggregateInstance;

impl RecordPaymentDeclineAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_payment_decline(&mut self, provider_reference: String, quote: RentalQuote) {
        self.raise(RentalPaymentDeclined {
            payment_id: self.state().payment_id().clone(),
            provider_reference,
            quote,
        });
    }
}
