use rostfrei::AggregateEvents;

use super::{
    BicycleAdded, BicycleCategorized, BicycleCollectionRequested, BicycleCollectionResolved,
    BicycleDamageReported, BicycleRented, BicycleReservationConfirmed, BicycleRetired,
    BicycleReturned, BicycleTransferredIn, BicycleTransferredOut, ReservedBicycleReturned,
};

#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    BicycleAdded(BicycleAdded),
    BicycleCategorized(BicycleCategorized),
    BicycleReservationConfirmed(BicycleReservationConfirmed),
    BicycleCollectionRequested(BicycleCollectionRequested),
    BicycleCollectionResolved(BicycleCollectionResolved),
    BicycleRented(BicycleRented),
    BicycleReturned(BicycleReturned),
    ReservedBicycleReturned(ReservedBicycleReturned),
    BicycleDamageReported(BicycleDamageReported),
    BicycleRetired(BicycleRetired),
    BicycleTransferredOut(BicycleTransferredOut),
    BicycleTransferredIn(BicycleTransferredIn),
}
