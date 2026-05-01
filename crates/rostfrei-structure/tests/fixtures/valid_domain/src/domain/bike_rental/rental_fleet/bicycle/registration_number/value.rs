#[derive(ValueObject)]
#[domain(id = "registration-number", label = "Registration number")]
pub struct RegistrationNumber(String);

impl RegistrationNumber {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
