use rostfrei::AggregateEvents;

use super::{BicycleAdded, BicycleRented, BicycleRetired, BicycleReturned, RentalFleetImported};

#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    RentalFleetImported(RentalFleetImported),
    BicycleAdded(BicycleAdded),
    BicycleRented(BicycleRented),
    BicycleReturned(BicycleReturned),
    BicycleRetired(BicycleRetired),
}
