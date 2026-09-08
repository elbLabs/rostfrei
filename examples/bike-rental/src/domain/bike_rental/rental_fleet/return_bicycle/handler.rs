use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{BicycleNotRented, ReturnBicycle, ReturnBicycleAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;

pub struct ReturnBicycleHandler;

#[async_trait]
impl CommandHandler<ReturnBicycle> for ReturnBicycleHandler {
    type Rejection = BicycleNotRented;

    async fn handle(
        &self,
        command: &ReturnBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        match fleet
            .aggregate_mut()
            .return_bicycle(command.bicycle_id.clone())
        {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
