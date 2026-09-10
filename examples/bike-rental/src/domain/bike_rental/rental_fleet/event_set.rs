use rostfrei::AggregateEvents;

use super::{
    BicycleAdded, BicycleRented, BicycleRetired, BicycleReturned, BicycleTransferredIn,
    BicycleTransferredOut,
};

#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    BicycleAdded(BicycleAdded),
    BicycleRented(BicycleRented),
    BicycleReturned(BicycleReturned),
    BicycleRetired(BicycleRetired),
    BicycleTransferredOut(BicycleTransferredOut),
    BicycleTransferredIn(BicycleTransferredIn),
}
