use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{AddBicycleAction, BicycleAlreadyInFleet, RegisterBicycle};
use crate::domain::rental_fleet::{AddCategorizedBicycleAction as _, RentalFleetAggregate};

pub struct RegisterBicycleHandler;

#[async_trait]
impl CommandHandler<RegisterBicycle> for RegisterBicycleHandler {
    type Rejection = BicycleAlreadyInFleet;

    async fn handle(
        &self,
        command: &RegisterBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        let result = match &command.category {
            Some(category) => fleet.add_categorized_bicycle(
                command.bicycle_id.clone(),
                command.condition,
                category.clone(),
            ),
            None => fleet.add_bicycle(command.bicycle_id.clone(), command.condition),
        };
        match result {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
