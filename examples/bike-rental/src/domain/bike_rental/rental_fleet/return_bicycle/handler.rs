use rostfrei::{AggregateInstance, CommandHandler};

use super::{BicycleNotRented, ReturnBicycle, ReturnBicycleActions};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl CommandHandler<ReturnBicycle> for RentalFleetAggregate {
    type Rejection = BicycleNotRented;

    fn handle(
        command: &ReturnBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.return_bicycle(command.bicycle_id.clone())
    }
}
