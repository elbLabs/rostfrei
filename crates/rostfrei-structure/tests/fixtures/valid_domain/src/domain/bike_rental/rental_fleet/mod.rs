mod aggregate;
mod bicycle;
mod bicycle_availability;
mod event_set;
mod fleet_consistency;
mod identity;
mod initialize;
mod rental_assessment;
mod rent_bicycle;
mod root;

pub use aggregate::RentalFleetAggregate;
pub use bicycle::{Bicycle, BicycleId, BicycleRentalStatus, BicycleStatus};
pub use identity::FleetId;
pub use rent_bicycle::{BicycleRented, BicycleUnavailable, RentBicycle};
pub use root::RentalFleet;
