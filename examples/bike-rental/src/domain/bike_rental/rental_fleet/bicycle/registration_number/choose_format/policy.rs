use rostfrei::domain_policy;

use super::RegistrationNumberFormat;

#[domain_policy(
    id = "choose-registration-number-format",
    label = "Choose registration number format"
)]
pub trait ChooseRegistrationNumberFormatPolicy {
    fn choose_format(&self) -> RegistrationNumberFormat;
}
