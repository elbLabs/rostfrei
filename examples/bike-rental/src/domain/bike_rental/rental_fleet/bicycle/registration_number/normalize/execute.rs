use super::{super::RegistrationNumber, NormalizeRegistrationNumber};

impl NormalizeRegistrationNumber for RegistrationNumber {
    fn normalize(&self) -> RegistrationNumber {
        Self::new(
            self.as_str()
                .split_whitespace()
                .map(str::to_ascii_uppercase)
                .collect::<Vec<_>>()
                .join("-"),
        )
    }
}
