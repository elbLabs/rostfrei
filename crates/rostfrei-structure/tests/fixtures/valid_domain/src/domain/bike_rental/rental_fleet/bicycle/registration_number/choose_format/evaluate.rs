use super::{ChooseRegistrationFormat, RegistrationFormat, RegistrationNumber};

impl ChooseRegistrationFormat for RegistrationNumber {
    fn choose_format(&self) -> RegistrationFormat {
        RegistrationFormat::Compact
    }
}
