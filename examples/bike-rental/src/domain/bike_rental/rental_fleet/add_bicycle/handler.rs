use std::convert::Infallible;

use rostfrei::{AggregateInstance, CommandHandler};

use super::{AddBicycle, AddBicycleAction as _};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl CommandHandler<AddBicycle> for RentalFleetAggregate {
    type Rejection = Infallible;

    fn handle(
        _command: &AddBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.add_bicycle();
        Ok(())
    }
}
