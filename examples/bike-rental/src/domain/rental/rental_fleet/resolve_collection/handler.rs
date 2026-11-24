use super::{ResolveBicycleCollection, ResolveCollectionAction as _};
use crate::{
    domain::rental_fleet::{CollectionRejected, RentalFleetAggregate},
    payments::is_integration_command,
};
use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

pub struct ResolveBicycleCollectionHandler;

#[async_trait]
impl CommandHandler<ResolveBicycleCollection> for ResolveBicycleCollectionHandler {
    type Rejection = CollectionRejected;

    async fn handle(
        &self,
        command: &ResolveBicycleCollection,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        if !is_integration_command(execution) {
            return Ok(CommandDecision::Rejected(CollectionRejected {
                reservation_id: command.reservation_id.clone(),
                reason: "integration-command-required".to_owned(),
            }));
        }
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        Ok(
            match fleet.resolve_collection(
                command.reservation_id.clone(),
                command.bicycle_id.clone(),
                command.payment_id.clone(),
                command.outcome,
            ) {
                Ok(()) => CommandDecision::Accepted,
                Err(error) => CommandDecision::Rejected(error),
            },
        )
    }
}
