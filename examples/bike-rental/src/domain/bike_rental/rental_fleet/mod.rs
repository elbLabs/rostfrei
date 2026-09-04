mod add_bicycle;
mod aggregate;
pub(in crate::domain) mod assess_rental_eligibility;
mod bicycle;
mod bicycle_availability;
mod event_set;
pub(in crate::domain) mod fleet_consistency;
mod identity;
mod import_rental_fleet;
mod initialize;
mod rent_bicycle;
mod return_bicycle;
mod root;
mod stream;

pub use add_bicycle::{AddBicycle, AddBicycleAction, BicycleAdded};
pub use aggregate::RentalFleetAggregate;
pub use bicycle::{
    Bicycle, BicycleCondition, BicycleId, BicycleRentalTransition, BicycleStatus,
    ChooseRegistrationNumberFormat, NormalizeRegistrationNumber, RegistrationNumber,
    RegistrationNumberFormat, RegistrationNumberValidity,
};
pub use bicycle_availability::{BicycleAvailability, BicycleAvailabilityQuery};
pub use event_set::RentalFleetEvent;
pub use identity::FleetId;
pub use import_rental_fleet::{
    ImportRentalFleetAction, ImportRentalFleetInput, ImportedBicycle, RentalFleetImported,
};
pub use rent_bicycle::{BicycleRented, BicycleUnavailable, RentBicycle, RentBicycleAction};
pub use return_bicycle::{BicycleNotRented, BicycleReturned, ReturnBicycle, ReturnBicycleAction};
pub use root::RentalFleet;
pub use stream::stream_id;
