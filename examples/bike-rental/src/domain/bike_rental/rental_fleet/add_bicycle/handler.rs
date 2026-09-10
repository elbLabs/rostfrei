use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{AddBicycle, AddBicycleAction, BicycleAlreadyInFleet};
use crate::domain::rental_fleet::RentalFleetAggregate;

pub struct AddBicycleHandler;

#[async_trait]
impl CommandHandler<AddBicycle> for AddBicycleHandler {
    type Rejection = BicycleAlreadyInFleet;

    async fn handle(
        &self,
        command: &AddBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        match fleet
            .aggregate_mut()
            .add_bicycle(command.bicycle_id.clone(), command.condition)
        {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
