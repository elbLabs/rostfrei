use rostfrei::domain_decision;

use super::RegistrationNumberFormat;

#[domain_decision(
    id = "choose-registration-number-format",
    label = "Choose registration number format"
)]
pub trait ChooseRegistrationNumberFormat {
    fn choose_format(&self) -> RegistrationNumberFormat;
}
