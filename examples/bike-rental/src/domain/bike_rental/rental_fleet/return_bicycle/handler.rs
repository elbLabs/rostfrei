use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{BicycleNotRented, ReturnBicycle, ReturnBicycleAction as _};
use crate::domain::rental_fleet::{
    RentalFleetAggregate, ReturnCollectedBicycleAction as _, ReturnDamagedBicycleAction as _,
};

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
        let result = if fleet
            .state()
            .active_reservation(&command.bicycle_id)
            .is_some()
        {
            fleet.return_collected_bicycle(
                command.bicycle_id.clone(),
                command.returned_at.clone(),
                command.damage_report.clone(),
            )
        } else {
            match &command.damage_report {
                Some(report) => {
                    fleet.return_damaged_bicycle(command.bicycle_id.clone(), report.clone())
                }
                None => fleet.return_bicycle(command.bicycle_id.clone()),
            }
        };
        match result {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
