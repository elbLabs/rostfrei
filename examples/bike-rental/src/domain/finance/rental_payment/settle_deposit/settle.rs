use super::RecordDepositRefundAction as _;
use crate::{
    domain::rental_payment::{PaymentOutcome, PaymentRejected, RentalPaymentAggregate},
    payments::PaymentProvider,
};
use rostfrei::AggregateInstance;

pub(in crate::domain::finance::rental_payment) async fn settle(
    payment: &mut AggregateInstance<RentalPaymentAggregate>,
    provider: &dyn PaymentProvider,
) -> Result<(), PaymentRejected> {
    let state = payment.state();
    let reject = |reason: &str| PaymentRejected {
        payment_id: state.payment_id().clone(),
        reason: reason.to_owned(),
    };
    if state.refund_reference().is_some() {
        return Ok(());
    }
    if state.outcome() != Some(PaymentOutcome::Approved) {
        return Err(reject("payment-not-approved"));
    }
    match state.return_eligible() {
        None => return Err(reject("rental-not-returned")),
        Some(false) => return Ok(()),
        Some(true) => {}
    }
    let details = state
        .details()
        .ok_or_else(|| reject("payment-not-requested"))?;
    let amount_minor = details
        .quote
        .security_deposit_minor
        .ok_or_else(|| reject("deposit-unknown"))?;
    if amount_minor == 0 {
        return Ok(());
    }
    let charge_reference = state
        .provider_reference()
        .ok_or_else(|| reject("missing-charge-reference"))?;
    let result = provider
        .refund_deposit(
            state.payment_id(),
            charge_reference,
            &details.quote.currency,
            amount_minor,
        )
        .await;
    if result.outcome != PaymentOutcome::Approved {
        return Err(reject("refund-declined"));
    }
    payment.record_deposit_refund(result.reference)
}
