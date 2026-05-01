use rostfrei::domain_action;

use super::super::RegistrationNumber;

#[domain_action(
    id = "normalize-registration-number",
    label = "Normalize registration number"
)]
pub trait NormalizeRegistrationNumber {
    fn normalize(&self) -> RegistrationNumber;
}
