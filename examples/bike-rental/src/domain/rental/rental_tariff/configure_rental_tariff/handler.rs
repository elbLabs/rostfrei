use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{ConfigureRentalTariff, ConfigureRentalTariffAction as _, InvalidRentalTariff};
use crate::domain::rental_tariff::RentalTariffAggregate;

pub struct ConfigureRentalTariffHandler;

#[async_trait]
impl CommandHandler<ConfigureRentalTariff> for ConfigureRentalTariffHandler {
    type Rejection = InvalidRentalTariff;

    async fn handle(
        &self,
        command: &ConfigureRentalTariff,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut tariff = execution
            .load::<RentalTariffAggregate>(command.category.as_str())
            .await?;
        match tariff.configure_rental_tariff(command.terms.clone()) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
