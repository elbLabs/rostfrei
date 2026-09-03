use super::{super::RegistrationNumber, ChooseRegistrationNumberFormat, RegistrationNumberFormat};

impl ChooseRegistrationNumberFormat for RegistrationNumber {
    fn choose_format(&self) -> RegistrationNumberFormat {
        if self.as_str().contains('-') {
            RegistrationNumberFormat::Segmented
        } else {
            RegistrationNumberFormat::Compact
        }
    }
}
