use super::{CollectRentalPayment, PaymentRejected, RecordPaymentResultAction as _};
use crate::{domain::rental_payment::RentalPaymentAggregate, payments::PaymentProvider};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};
use std::sync::Arc;

pub struct CollectRentalPaymentHandler {
    pub provider: Arc<dyn PaymentProvider>,
}

#[async_trait]
impl CommandHandler<CollectRentalPayment> for CollectRentalPaymentHandler {
    type Rejection = PaymentRejected;
    async fn handle(
        &self,
        command: &CollectRentalPayment,
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
        if payment.state().outcome().is_some() {
            return Ok(CommandDecision::Accepted);
        }
        let Some(details) = payment.state().details().cloned() else {
            return Ok(reject("payment-not-requested"));
        };
        let result = self.provider.charge(&command.payment_id, &details).await;
        Ok(
            match payment.record_payment_result(result.outcome, result.reference) {
                Ok(()) => CommandDecision::Accepted,
                Err(error) => CommandDecision::Rejected(error),
            },
        )
    }
}
