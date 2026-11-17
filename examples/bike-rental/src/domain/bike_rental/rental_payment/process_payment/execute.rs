use super::{PaymentRejected, RecordPaymentResultAction};
use crate::domain::rental_payment::{
    PaymentOutcome, RecordPaymentApprovalAction as _, RecordPaymentDeclineAction as _,
    RentalPaymentAggregate,
};
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
        let quote = details.quote.clone();
        match outcome {
            PaymentOutcome::Approved => self.record_payment_approval(provider_reference, quote),
            PaymentOutcome::Declined => self.record_payment_decline(provider_reference, quote),
        }
        Ok(())
    }
}
