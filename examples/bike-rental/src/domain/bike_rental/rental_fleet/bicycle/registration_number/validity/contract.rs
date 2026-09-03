use rostfrei::{InvariantViolation, domain_invariant};

#[domain_invariant(
    id = "registration-number-validity",
    label = "Registration number validity"
)]
pub trait RegistrationNumberValidity {
    fn validate(&self) -> Option<InvariantViolation>;
}
