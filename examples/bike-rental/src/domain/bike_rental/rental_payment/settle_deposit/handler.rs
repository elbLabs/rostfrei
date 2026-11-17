use super::{RecordDepositRefundAction as _, RefundEligibleRentalDeposit};
use crate::{
    domain::{
        rental_fleet::RentalFleetAggregate,
        rental_payment::{PaymentOutcome, PaymentRejected, RentalPaymentAggregate},
    },
    payments::PaymentProvider,
};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};
use std::sync::Arc;

pub struct RefundEligibleRentalDepositHandler {
    pub provider: Arc<dyn PaymentProvider>,
}

#[async_trait]
impl CommandHandler<RefundEligibleRentalDeposit> for RefundEligibleRentalDepositHandler {
    type Rejection = PaymentRejected;
    async fn handle(
        &self,
        command: &RefundEligibleRentalDeposit,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let reject = |reason: &str| {
            CommandDecision::Rejected(PaymentRejected {
                payment_id: command.payment_id.clone(),
                reason: reason.to_owned(),
            })
        };
        let mut payment = execution
            .load::<RentalPaymentAggregate>(command.payment_id.as_str())
            .await?;
        if payment.state().refund_reference().is_some() {
            return Ok(CommandDecision::Accepted);
        }
        if payment.state().outcome() != Some(PaymentOutcome::Approved) {
            return Ok(reject("payment-not-approved"));
        }
        let Some(details) = payment.state().details().cloned() else {
            return Ok(reject("payment-not-requested"));
        };
        let fleet = execution
            .load::<RentalFleetAggregate>(details.fleet_id.as_str())
            .await?;
        let Some(reservation) = fleet
            .state()
            .reservations()
            .iter()
            .find(|reservation| reservation.reservation_id() == &details.reservation_id)
        else {
            return Ok(reject("reservation-not-found"));
        };
        let Some(collection) = reservation.collection().filter(|collection| {
            collection.collected
                && collection.returned
                && collection.payment_id == command.payment_id
                && collection.bicycle_id == details.bicycle_id
        }) else {
            return Ok(reject("rental-not-returned"));
        };
        if collection
            .return_details
            .as_ref()
            .is_none_or(|returned| !returned.permits_full_refund(reservation.period()))
        {
            return Ok(CommandDecision::Accepted);
        }
        let Some(amount_minor) = details.quote.security_deposit_minor else {
            return Ok(reject("deposit-unknown"));
        };
        if amount_minor == 0 {
            return Ok(CommandDecision::Accepted);
        }
        let Some(charge_reference) = payment.state().provider_reference() else {
            return Ok(reject("missing-charge-reference"));
        };
        let result = self
            .provider
            .refund_deposit(
                &command.payment_id,
                charge_reference,
                &details.quote.currency,
                amount_minor,
            )
            .await;
        if result.outcome != PaymentOutcome::Approved {
            return Ok(reject("refund-declined"));
        }
        match payment.record_deposit_refund(result.reference) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(error) => Ok(CommandDecision::Rejected(error)),
        }
    }
}
