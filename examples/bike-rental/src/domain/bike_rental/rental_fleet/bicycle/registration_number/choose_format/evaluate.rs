use super::{
    super::RegistrationNumber, ChooseRegistrationNumberFormatPolicy, RegistrationNumberFormat,
};

impl ChooseRegistrationNumberFormatPolicy for RegistrationNumber {
    fn choose_format(&self) -> RegistrationNumberFormat {
        if self.as_str().contains('-') {
            RegistrationNumberFormat::Segmented
        } else {
            RegistrationNumberFormat::Compact
        }
    }
}
