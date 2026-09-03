#[domain_action(id = "normalize-registration-number", label = "Normalize registration number")]
pub trait NormalizeRegistrationNumber {
    fn normalize(&mut self);
}
