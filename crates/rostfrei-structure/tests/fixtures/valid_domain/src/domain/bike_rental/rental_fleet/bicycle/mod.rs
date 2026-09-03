mod entity;
mod identity;
mod mark_rented;
mod rental_status;
mod roadworthiness;
mod registration_number;
mod status;

pub use entity::Bicycle;
pub use identity::BicycleId;
pub use rental_status::BicycleRentalStatus;
pub use registration_number::RegistrationNumber;
pub use status::BicycleStatus;
