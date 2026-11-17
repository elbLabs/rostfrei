use async_trait::async_trait;
use rostfrei::{CommandDecision, CommandExecution, CommandHandler, CommandHandlingResult};

use super::{ReservationRejected, ReserveBicycleCategory, ReserveBicycleCategoryAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;
use crate::domain::rental_tariff::RentalTariffAggregate;

pub struct ReserveBicycleCategoryHandler;

#[async_trait]
impl CommandHandler<ReserveBicycleCategory> for ReserveBicycleCategoryHandler {
    type Rejection = ReservationRejected;

    async fn handle(
        &self,
        command: &ReserveBicycleCategory,
        execution: &mut CommandExecution<'_>,
    ) -> CommandHandlingResult<Self::Rejection> {
        let mut fleet = execution
            .load::<RentalFleetAggregate>(command.fleet_id.as_str())
            .await?;
        let quote = if let Some(existing) = fleet
            .state()
            .reservations()
            .iter()
            .find(|reservation| reservation.reservation_id() == &command.reservation_id)
        {
            existing.quote().cloned()
        } else {
            let tariff = execution
                .load::<RentalTariffAggregate>(command.category.as_str())
                .await?;
            tariff
                .state()
                .terms()
                .and_then(|terms| terms.quote(&command.period))
        };
        match fleet.reserve_bicycle_category(
            command.reservation_id.clone(),
            command.customer_id.clone(),
            command.category.clone(),
            command.period.clone(),
            quote,
        ) {
            Ok(()) => Ok(CommandDecision::Accepted),
            Err(rejection) => Ok(CommandDecision::Rejected(rejection)),
        }
    }
}
