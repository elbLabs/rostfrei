mod condition;
mod entity;
mod identity;
pub(super) mod mark_available;
pub(super) mod mark_rented;
mod registration_number;
mod rental_status;

pub use condition::BicycleCondition;
pub use entity::Bicycle;
pub use identity::BicycleId;
pub use registration_number::{
    ChooseRegistrationNumberFormat, NormalizeRegistrationNumber, RegistrationNumber,
    RegistrationNumberFormat, RegistrationNumberValidity,
};
pub use rental_status::{BicycleRentalTransition, BicycleStatus};
