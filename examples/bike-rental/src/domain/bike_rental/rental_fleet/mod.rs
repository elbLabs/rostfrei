mod add_bicycle;
mod add_categorized_bicycle;
mod aggregate;
mod bicycle;
mod bicycle_availability;
mod cancel_collection;
mod collect_reserved_bicycle;
mod complete_collection;
mod event_set;
pub(in crate::domain) mod fleet_consistency;
mod identity;
mod initialize;
mod rent_bicycle;
mod reservation;
mod reserve_bicycle_category;
mod resolve_collection;
mod retire_bicycle;
mod return_bicycle;
mod return_collected_bicycle;
mod return_damaged_bicycle;
mod root;
mod stream;
mod transfer_bicycle_in;
mod transfer_bicycle_out;

pub use super::bicycle_transfer::{
    BicycleTransfer, BicycleTransferAction, BicycleTransferRejected,
    BicycleTransferRejectionReason, TransferBicycle, TransferBicycleHandler,
};
pub use add_bicycle::{
    AddBicycleAction, BicycleAdded, BicycleAlreadyInFleet, RegisterBicycle, RegisterBicycleHandler,
};
pub use add_categorized_bicycle::{AddCategorizedBicycleAction, BicycleCategorized};
pub use aggregate::RentalFleetAggregate;
pub(in crate::domain) use bicycle::assess_rental_eligibility;
pub use bicycle::{
    Bicycle, BicycleCategory, BicycleCondition, BicycleId, BicycleRentalTransition, BicycleStatus,
    ChooseRegistrationNumberFormatPolicy, NormalizeRegistrationNumber, RegistrationNumber,
    RegistrationNumberFormat, RegistrationNumberValidity,
};
pub use bicycle_availability::{BicycleAvailability, BicycleAvailabilityQuery};
pub use cancel_collection::BicycleCollectionCancelled;
use cancel_collection::CancelCollectionAction;
pub use collect_reserved_bicycle::{
    BicycleCollectionRequested, CollectReservedBicycle, CollectReservedBicycleAction,
    CollectReservedBicycleHandler, CollectionRejected,
};
use complete_collection::CompleteCollectionAction;
pub use complete_collection::ReservedBicycleCollected;
pub use event_set::RentalFleetEvent;
pub use identity::FleetId;
pub use rent_bicycle::{BicycleRented, BicycleUnavailable, RentBicycleAction};
pub use reservation::{
    CollectionAssignment, RentalPeriod, RentalReturn, Reservation, ReservationId, ReturnTime,
};
pub use reserve_bicycle_category::{
    BicycleReservationConfirmed, ReservationRejected, ReserveBicycleCategory,
    ReserveBicycleCategoryAction, ReserveBicycleCategoryHandler,
};
pub use resolve_collection::{BicycleCollectionResolved, ResolveCollectionAction};
pub use retire_bicycle::{BicycleCannotBeRetired, BicycleRetired, RetireBicycleAction};
pub use return_bicycle::{
    BicycleNotRented, BicycleReturned, ReturnBicycle, ReturnBicycleAction, ReturnBicycleHandler,
};
pub use return_collected_bicycle::{ReservedBicycleReturned, ReturnCollectedBicycleAction};
pub use return_damaged_bicycle::{BicycleDamageReported, ReturnDamagedBicycleAction};
pub use root::RentalFleet;
pub use stream::stream_id;
pub use transfer_bicycle_in::{BicycleTransferredIn, TransferBicycleInAction};
pub use transfer_bicycle_out::{BicycleTransferredOut, TransferBicycleOutAction};
