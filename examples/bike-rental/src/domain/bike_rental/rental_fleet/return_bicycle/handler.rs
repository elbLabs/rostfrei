use rostfrei::{AggregateInstance, CommandHandler, CommandType};

use super::{ReturnBicycle, ReturnBicycleActions};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl CommandHandler<ReturnBicycle> for RentalFleetAggregate {
    type Rejection = <ReturnBicycle as CommandType>::Rejection;

    fn handle(
        command: &ReturnBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.return_bicycle(command.bicycle_id.clone())
    }
}
