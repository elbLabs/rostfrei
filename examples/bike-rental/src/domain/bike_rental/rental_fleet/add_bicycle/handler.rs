use std::convert::Infallible;

use async_trait::async_trait;
use rostfrei::{
    AggregateInstance, CommandContext, CommandDecision, CommandHandler, CommandHandlingResult,
};

use super::{AddBicycle, AddBicycleAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[async_trait]
impl CommandHandler<AddBicycle> for RentalFleetAggregate {
    type Rejection = Infallible;

    async fn handle(
        _command: &AddBicycle,
        aggregate: &mut AggregateInstance<Self>,
        _context: &mut CommandContext<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        aggregate.add_bicycle();
        Ok(CommandDecision::Accepted)
    }
}
