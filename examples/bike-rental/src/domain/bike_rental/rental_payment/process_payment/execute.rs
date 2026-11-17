use super::{PaymentRejected, RecordPaymentResultAction, RentalPaymentProcessed};
use crate::domain::rental_payment::{PaymentOutcome, RentalPaymentAggregate};
use rostfrei::AggregateInstance;

impl RecordPaymentResultAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_payment_result(
        &mut self,
        outcome: PaymentOutcome,
        provider_reference: String,
    ) -> Result<(), PaymentRejected> {
        let reject = |reason: &str| PaymentRejected {
            payment_id: self.state().payment_id().clone(),
            reason: reason.to_owned(),
        };
        if let Some(existing) = self.state().outcome() {
            return if existing == outcome
                && self.state().provider_reference() == Some(provider_reference.as_str())
            {
                Ok(())
            } else {
                Err(reject("payment-already-processed"))
            };
        }
        let details = self
            .state()
            .details()
            .ok_or_else(|| reject("payment-not-requested"))?;
        if provider_reference.trim().is_empty() {
            return Err(reject("missing-provider-reference"));
        }
        self.raise(RentalPaymentProcessed {
            payment_id: self.state().payment_id().clone(),
            outcome,
            provider_reference,
            quote: details.quote.clone(),
        });
        Ok(())
    }
}
