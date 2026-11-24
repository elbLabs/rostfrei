use super::{CollectReservedBicycle, CollectReservedBicycleAction as _, CollectionRejected};
use crate::domain::{
    rental_fleet::RentalFleetAggregate,
    rental_payment::{RentalPaymentAggregate, RequestRentalPaymentAction as _},
};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

pub struct CollectReservedBicycleHandler;

#[async_trait]
impl CommandHandler<CollectReservedBicycle> for CollectReservedBicycleHandler {
    type Rejection = CollectionRejected;
    async fn handle(
        &self,
        command: &CollectReservedBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut payment = execution
            .load::<RentalPaymentAggregate>(command.payment_id.as_str())
            .await?;
        if let Some(existing) = payment.state().details() {
            return Ok(
                if existing.fleet_id == command.fleet_id
                    && existing.reservation_id == command.reservation_id
                    && existing.bicycle_id == command.bicycle_id
                    && existing.payment_method == command.payment_method
                {
                    CommandDecision::Accepted
                } else {
                    CommandDecision::Rejected(CollectionRejected {
                        reservation_id: command.reservation_id.clone(),
                        reason: "payment-identity-conflict".to_owned(),
                    })
                },
            );
        }
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        let details = match fleet.collect_reserved_bicycle(
            command.reservation_id.clone(),
            command.bicycle_id.clone(),
            command.payment_id.clone(),
            command.payment_method.clone(),
        ) {
            Ok(details) => details,
            Err(rejection) => return Ok(CommandDecision::Rejected(rejection)),
        };
        match payment.request_rental_payment(details) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(CollectionRejected {
                reservation_id: command.reservation_id.clone(),
                reason: rejection.reason,
            })),
        }
    }
}
