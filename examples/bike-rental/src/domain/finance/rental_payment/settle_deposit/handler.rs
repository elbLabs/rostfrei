use super::{RefundEligibleRentalDeposit, settle};
use crate::{
    domain::rental_payment::{PaymentRejected, RentalPaymentAggregate},
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
        let mut payment = execution
            .load::<RentalPaymentAggregate>(command.payment_id.as_str())
            .await?;
        Ok(match settle(&mut payment, self.provider.as_ref()).await {
            Ok(()) => CommandDecision::Accepted,
            Err(error) => CommandDecision::Rejected(error),
        })
    }
}
