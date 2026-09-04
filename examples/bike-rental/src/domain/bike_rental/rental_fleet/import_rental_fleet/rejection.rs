use rostfrei::{DomainError, InvariantViolation};

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "invalid-rental-fleet",
    label = "Invalid rental fleet",
    code = "INVALID_RENTAL_FLEET",
    message = "The rental fleet violates a domain invariant."
)]
pub struct InvalidRentalFleet {
    pub path: String,
    pub reason: String,
}

impl From<InvariantViolation> for InvalidRentalFleet {
    fn from(violation: InvariantViolation) -> Self {
        Self {
            path: violation.path,
            reason: violation.reason,
        }
    }
}
