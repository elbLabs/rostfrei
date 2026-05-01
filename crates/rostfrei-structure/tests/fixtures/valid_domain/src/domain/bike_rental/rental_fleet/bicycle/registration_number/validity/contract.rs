#[domain_invariant(id = "registration-number-validity", label = "Registration number validity")]
pub trait RegistrationNumberValidity {
    fn is_valid(&self) -> bool;
}
