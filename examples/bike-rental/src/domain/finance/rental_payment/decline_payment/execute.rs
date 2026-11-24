use super::{RecordPaymentDeclineAction, RentalPaymentDeclined};
use crate::domain::rental_payment::{CollectionPayment, RentalPaymentAggregate};
use rostfrei::AggregateInstance;

impl RecordPaymentDeclineAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_payment_decline(&mut self, provider_reference: String, details: CollectionPayment) {
        self.raise(RentalPaymentDeclined {
            payment_id: self.state().payment_id().clone(),
            fleet_id: details.fleet_id,
            reservation_id: details.reservation_id,
            bicycle_id: details.bicycle_id,
            provider_reference,
            quote: details.quote,
        });
    }
}
