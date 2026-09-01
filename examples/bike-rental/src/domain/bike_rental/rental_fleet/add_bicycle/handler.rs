use rostfrei::{AggregateInstance, CommandHandler, CommandType};

use super::{AddBicycle, AddBicycleActions};
use crate::domain::rental_fleet::RentalFleetAggregate;

impl CommandHandler<AddBicycle> for RentalFleetAggregate {
    type Rejection = <AddBicycle as CommandType>::Rejection;

    fn handle(
        _command: &AddBicycle,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.add_bicycle();
        Ok(())
    }
}
