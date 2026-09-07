use async_trait::async_trait;
use rostfrei::{
    AggregateInstance, CommandContext, CommandDecision, CommandHandler, CommandHandlingResult,
};

use super::{BicycleTransfer, BicycleTransferRejected, TransferBicycle};
use crate::domain::rental_fleet::RentalFleetAggregate;

#[async_trait]
#[allow(
    clippy::use_self,
    reason = "the example names the additional aggregate type explicitly"
)]
impl CommandHandler<TransferBicycle> for RentalFleetAggregate {
    type Rejection = BicycleTransferRejected;

    async fn handle(
        command: &TransferBicycle,
        source: &mut AggregateInstance<Self>,
        context: &mut CommandContext<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        if let Err(rejection) = BicycleTransfer::validate_route(source, command) {
            return Ok(CommandDecision::Rejected(rejection));
        }

        let mut destination = context
            .load::<RentalFleetAggregate>(command.to_fleet_id.as_str())
            .await?;

        match BicycleTransfer::transfer(source, destination.aggregate_mut(), command) {
            Ok(()) => {
                context.include(destination)?;
                Ok(CommandDecision::Accepted)
            }
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
