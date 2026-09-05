pub(in crate::domain) mod assess_rental_eligibility;
mod condition;
mod entity;
mod identity;
mod registration_number;
mod rental_status;

pub use condition::BicycleCondition;
pub use entity::Bicycle;
pub use identity::BicycleId;
pub use registration_number::{
    ChooseRegistrationNumberFormatPolicy, NormalizeRegistrationNumber, RegistrationNumber,
    RegistrationNumberFormat, RegistrationNumberValidity,
};
pub use rental_status::{BicycleRentalTransition, BicycleStatus};
