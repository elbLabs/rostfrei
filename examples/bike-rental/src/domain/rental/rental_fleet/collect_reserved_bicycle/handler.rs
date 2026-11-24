use super::{CollectReservedBicycle, CollectReservedBicycleAction as _, CollectionRejected};
use crate::domain::rental_fleet::RentalFleetAggregate;
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
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        Ok(
            match fleet.collect_reserved_bicycle(
                command.reservation_id.clone(),
                command.bicycle_id.clone(),
                command.payment_id.clone(),
                command.payment_method.clone(),
            ) {
                Ok(_) => CommandDecision::Accepted,
                Err(rejection) => CommandDecision::Rejected(rejection),
            },
        )
    }
}
