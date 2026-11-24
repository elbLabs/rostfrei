use super::{PaymentId, RentalPayment, RentalPaymentAggregate};
use rostfrei::{Initialize, StreamId};

impl Initialize<RentalPaymentAggregate> for RentalPayment {
    fn initialize(stream: &StreamId) -> Self {
        Self {
            payment_id: PaymentId::from(stream.aggregate_id()),
            details: None,
            outcome: None,
            provider_reference: None,
            refund_reference: None,
            return_eligible: None,
            conflicting_fleets: Vec::new(),
        }
    }
}
