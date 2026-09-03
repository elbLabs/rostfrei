use super::{RegistrationNumber, RegistrationNumberValidity};

impl RegistrationNumberValidity for RegistrationNumber {
    fn is_valid(&self) -> bool {
        true
    }
}
