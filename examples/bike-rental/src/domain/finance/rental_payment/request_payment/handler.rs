use super::{RecordRentalPaymentRequest, RequestRentalPaymentAction as _};
use crate::{
    domain::rental_payment::{PaymentRejected, RentalPaymentAggregate},
    payments::is_integration_command,
};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

pub struct RecordRentalPaymentRequestHandler;

#[async_trait]
impl CommandHandler<RecordRentalPaymentRequest> for RecordRentalPaymentRequestHandler {
    type Rejection = PaymentRejected;

    async fn handle(
        &self,
        command: &RecordRentalPaymentRequest,
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
        Ok(
            match payment.request_rental_payment(command.details.clone()) {
                Ok(()) => CommandDecision::Accepted,
                Err(error) => CommandDecision::Rejected(error),
            },
        )
    }
}
