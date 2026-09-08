use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{BicycleUnavailable, RentBicycle, RentBicycleAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;

pub struct RentBicycleHandler;

#[async_trait]
impl CommandHandler<RentBicycle> for RentBicycleHandler {
    type Rejection = BicycleUnavailable;

    async fn handle(
        &self,
        command: &RentBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        match fleet
            .aggregate_mut()
            .rent_bicycle(command.bicycle_id.clone())
        {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
