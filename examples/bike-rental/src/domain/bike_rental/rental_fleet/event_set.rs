use rostfrei::AggregateEvents;

use super::{BicycleAdded, BicycleRented, BicycleReturned, RentalFleetImported};

#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    RentalFleetImported(RentalFleetImported),
    BicycleAdded(BicycleAdded),
    BicycleRented(BicycleRented),
    BicycleReturned(BicycleReturned),
}
