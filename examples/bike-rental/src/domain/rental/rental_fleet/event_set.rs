use rostfrei::AggregateEvents;

use super::{
    BicycleAdded, BicycleCategorized, BicycleCollectionCancelled, BicycleCollectionRequested,
    BicycleCollectionResolved, BicycleDamageReported, BicycleRented, BicycleReservationConfirmed,
    BicycleRetired, BicycleReturned, BicycleTransferredIn, BicycleTransferredOut,
    ReservedBicycleCollected, ReservedBicycleReturned,
};

#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    BicycleAdded(BicycleAdded),
    BicycleCategorized(BicycleCategorized),
    BicycleReservationConfirmed(BicycleReservationConfirmed),
    BicycleCollectionRequested(BicycleCollectionRequested),
    BicycleCollectionResolved(BicycleCollectionResolved),
    ReservedBicycleCollected(ReservedBicycleCollected),
    BicycleCollectionCancelled(BicycleCollectionCancelled),
    BicycleRented(BicycleRented),
    BicycleReturned(BicycleReturned),
    ReservedBicycleReturned(ReservedBicycleReturned),
    BicycleDamageReported(BicycleDamageReported),
    BicycleRetired(BicycleRetired),
    BicycleTransferredOut(BicycleTransferredOut),
    BicycleTransferredIn(BicycleTransferredIn),
}
