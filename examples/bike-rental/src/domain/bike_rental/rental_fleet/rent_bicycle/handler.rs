use rostfrei::{AggregateInstance, CommandHandler};

use super::{BicycleUnavailable, RentBicycle, RentBicycleActions};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = BicycleUnavailable;

    fn handle(
        command: &RentBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.rent_bicycle(command.bicycle_id.clone())
    }
}
