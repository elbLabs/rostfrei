use rostfrei::InvariantViolation;

use super::{super::RegistrationNumber, RegistrationNumberValidity};

impl RegistrationNumberValidity for RegistrationNumber {
    fn validate(&self) -> Option<InvariantViolation> {
        let value = self.as_str();
        let valid = !value.is_empty()
            && value.split('-').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            });
        (!valid).then(|| {
            InvariantViolation::new(
                "registration_number",
                "must use uppercase letters and digits separated by single hyphens",
            )
        })
    }
}
