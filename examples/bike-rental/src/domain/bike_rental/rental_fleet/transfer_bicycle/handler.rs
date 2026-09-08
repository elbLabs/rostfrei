use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{BicycleTransfer, BicycleTransferRejected, TransferBicycle};
use crate::domain::rental_fleet::RentalFleetAggregate;

pub struct TransferBicycleHandler;

#[async_trait]
impl CommandHandler<TransferBicycle> for TransferBicycleHandler {
    type Rejection = BicycleTransferRejected;

    async fn handle(
        &self,
        command: &TransferBicycle,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut source = execution
            .load::<RentalFleetAggregate>(command.from_fleet_id.as_str())
            .await?;
        if let Err(rejection) = BicycleTransfer::validate_route(source.aggregate(), command) {
            return Ok(CommandDecision::Rejected(rejection));
        }
        let mut destination = execution
            .load::<RentalFleetAggregate>(command.to_fleet_id.as_str())
            .await?;
        match BicycleTransfer::transfer(
            source.aggregate_mut(),
            destination.aggregate_mut(),
            command,
        ) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
