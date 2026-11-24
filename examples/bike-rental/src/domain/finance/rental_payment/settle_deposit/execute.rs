use super::{RecordDepositRefundAction, SecurityDepositRefunded};
use crate::domain::rental_payment::{PaymentOutcome, PaymentRejected, RentalPaymentAggregate};
use rostfrei::AggregateInstance;

impl RecordDepositRefundAction for AggregateInstance<RentalPaymentAggregate> {
    fn record_deposit_refund(&mut self, refund_reference: String) -> Result<(), PaymentRejected> {
        let reject = |reason: &str| PaymentRejected {
            payment_id: self.state().payment_id().clone(),
            reason: reason.to_owned(),
        };
        if let Some(existing) = self.state().refund_reference() {
            return if existing == refund_reference {
                Ok(())
            } else {
                Err(reject("deposit-already-refunded"))
            };
        }
        if self.state().outcome() != Some(PaymentOutcome::Approved) {
            return Err(reject("payment-not-approved"));
        }
        if self.state().return_eligible() != Some(true) {
            return Err(reject("return-not-eligible"));
        }
        let details = self
            .state()
            .details()
            .ok_or_else(|| reject("payment-not-requested"))?;
        let amount_minor = details
            .quote
            .security_deposit_minor
            .filter(|amount| *amount > 0)
            .ok_or_else(|| reject("no-refundable-deposit"))?;
        let charge_reference = self
            .state()
            .provider_reference()
            .ok_or_else(|| reject("missing-charge-reference"))?
            .to_owned();
        if refund_reference.trim().is_empty() {
            return Err(reject("missing-refund-reference"));
        }
        self.raise(SecurityDepositRefunded {
            payment_id: self.state().payment_id().clone(),
            currency: details.quote.currency.clone(),
            amount_minor,
            charge_reference,
            refund_reference,
        });
        Ok(())
    }
}
