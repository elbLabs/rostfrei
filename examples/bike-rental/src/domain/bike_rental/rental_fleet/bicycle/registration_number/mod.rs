mod choose_format;
mod normalize;
mod validity;
mod value;

pub use choose_format::{ChooseRegistrationNumberFormat, RegistrationNumberFormat};
pub use normalize::NormalizeRegistrationNumber;
pub use validity::RegistrationNumberValidity;
pub use value::RegistrationNumber;
