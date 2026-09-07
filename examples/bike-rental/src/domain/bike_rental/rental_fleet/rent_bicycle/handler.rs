use async_trait::async_trait;
use rostfrei::{
    AggregateInstance, CommandContext, CommandDecision, CommandHandler, CommandHandlingResult,
};

use super::{BicycleUnavailable, RentBicycle, RentBicycleAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[async_trait]
impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = BicycleUnavailable;

    async fn handle(
        command: &RentBicycle,
        aggregate: &mut AggregateInstance<Self>,
        _context: &mut CommandContext<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        Ok(match aggregate.rent_bicycle(command.bicycle_id.clone()) {
            Ok(()) => CommandDecision::Accepted,
            Err(rejection) => CommandDecision::Rejected(rejection),
        })
    }
}
