use std::sync::Arc;

use super::{AssessRentalReturn, RecordRentalReturnEligibilityAction as _};
use crate::{
    domain::rental_payment::{PaymentRejected, RentalPaymentAggregate, settle_deposit::settle},
    payments::{PaymentProvider, is_integration_command},
};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

pub struct AssessRentalReturnHandler {
    pub provider: Arc<dyn PaymentProvider>,
}

#[async_trait]
impl CommandHandler<AssessRentalReturn> for AssessRentalReturnHandler {
    type Rejection = PaymentRejected;

    async fn handle(
        &self,
        command: &AssessRentalReturn,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        if !is_integration_command(execution) {
            return Ok(CommandDecision::Rejected(PaymentRejected {
                payment_id: command.payment_id.clone(),
                reason: "integration-command-required".to_owned(),
            }));
        }
        let mut payment = execution
            .load::<RentalPaymentAggregate>(command.payment_id.as_str())
            .await?;
        if let Err(error) = payment.record_rental_return_eligibility(
            &command.fleet_id,
            &command.reservation_id,
            &command.bicycle_id,
            command.eligible,
        ) {
            return Ok(CommandDecision::Rejected(error));
        }
        // Assessment is a fact even when settlement cannot complete. Keep it so
        // finance can attempt settlement again with the same refund key.
        let _ = settle(&mut payment, self.provider.as_ref()).await;
        Ok(CommandDecision::Accepted)
    }
}
