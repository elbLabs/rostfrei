use async_trait::async_trait;
use rostfrei::{
    AggregateInstance, CommandContext, CommandDecision, CommandHandler, CommandHandlingResult,
};

use super::{BicycleNotRented, ReturnBicycle, ReturnBicycleAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[async_trait]
impl CommandHandler<ReturnBicycle> for RentalFleetAggregate {
    type Rejection = BicycleNotRented;

    async fn handle(
        command: &ReturnBicycle,
        aggregate: &mut AggregateInstance<Self>,
        _context: &mut CommandContext<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        Ok(match aggregate.return_bicycle(command.bicycle_id.clone()) {
            Ok(()) => CommandDecision::Accepted,
            Err(rejection) => CommandDecision::Rejected(rejection),
        })
    }
}
