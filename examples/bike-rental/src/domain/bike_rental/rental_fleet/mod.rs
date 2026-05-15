mod add_bicycle;
mod aggregate;
mod bicycle;
mod bicycle_availability;
mod event_set;
pub(in crate::domain) mod fleet_consistency;
mod identity;
mod import_rental_fleet;
mod initialize;
mod rent_bicycle;
mod retire_bicycle;
mod return_bicycle;
mod root;
mod stream;
mod transfer_bicycle_in;
mod transfer_bicycle_out;

pub use super::bicycle_transfer::{
    BicycleTransfer, BicycleTransferAction, BicycleTransferRejected,
    BicycleTransferRejectionReason, TransferBicycle, TransferBicycleHandler,
};
pub use add_bicycle::{AddBicycle, AddBicycleAction, AddBicycleHandler, BicycleAdded};
pub use aggregate::RentalFleetAggregate;
pub(in crate::domain) use bicycle::assess_rental_eligibility;
pub use bicycle::{
    Bicycle, BicycleCondition, BicycleId, BicycleRentalTransition, BicycleStatus,
    ChooseRegistrationNumberFormatPolicy, NormalizeRegistrationNumber, RegistrationNumber,
    RegistrationNumberFormat, RegistrationNumberValidity,
};
pub use bicycle_availability::{BicycleAvailability, BicycleAvailabilityQuery};
pub use event_set::RentalFleetEvent;
pub use identity::FleetId;
pub use import_rental_fleet::{
    ImportRentalFleetAction, ImportRentalFleetInput, ImportedBicycle, InvalidRentalFleet,
    RentalFleetImported,
};
pub use rent_bicycle::{
    BicycleRented, BicycleUnavailable, RentBicycle, RentBicycleAction, RentBicycleHandler,
};
pub use retire_bicycle::{BicycleCannotBeRetired, BicycleRetired, RetireBicycleAction};
pub use return_bicycle::{
    BicycleNotRented, BicycleReturned, ReturnBicycle, ReturnBicycleAction, ReturnBicycleHandler,
};
pub use root::RentalFleet;
pub use stream::stream_id;
pub use transfer_bicycle_in::{BicycleTransferredIn, TransferBicycleInAction};
pub use transfer_bicycle_out::{BicycleTransferredOut, TransferBicycleOutAction};
