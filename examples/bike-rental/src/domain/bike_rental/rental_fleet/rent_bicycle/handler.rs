use rostfrei::{AggregateInstance, CommandHandler, CommandType};

use super::{RentBicycle, RentBicycleActions};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl CommandHandler<RentBicycle> for RentalFleetAggregate {
    type Rejection = <RentBicycle as CommandType>::Rejection;

    fn handle(
        command: &RentBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.rent_bicycle(command.bicycle_id.clone())
    }
}
