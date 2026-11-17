use super::{PaymentRejected, ProcessRentalPayment, RecordPaymentResultAction as _};
use crate::{
    domain::{
        rental_fleet::{RentalFleetAggregate, ResolveCollectionAction as _},
        rental_payment::RentalPaymentAggregate,
    },
    payments::PaymentProvider,
};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};
use std::sync::Arc;

pub struct ProcessRentalPaymentHandler {
    pub provider: Arc<dyn PaymentProvider>,
}

#[async_trait]
impl CommandHandler<ProcessRentalPayment> for ProcessRentalPaymentHandler {
    type Rejection = PaymentRejected;
    async fn handle(
        &self,
        command: &ProcessRentalPayment,
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
        let mut fleet = execution
            .load::<RentalFleetAggregate>(details.fleet_id.as_str())
            .await?;
        if fleet
            .state()
            .pending_collection(&details.reservation_id, &command.payment_id)
            .is_none_or(|collection| collection.bicycle_id != details.bicycle_id)
        {
            return Ok(reject("collection-not-pending"));
        }
        let result = self.provider.charge(&command.payment_id, &details).await;
        if let Err(error) = payment.record_payment_result(result.outcome, result.reference) {
            return Ok(CommandDecision::Rejected(error));
        }
        match fleet.resolve_collection(
            details.reservation_id,
            command.payment_id.clone(),
            result.outcome,
        ) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(error) => Ok(reject(&error.reason)),
        }
    }
}
