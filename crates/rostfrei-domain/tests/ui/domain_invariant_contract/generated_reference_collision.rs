#![allow(non_snake_case, unused_imports)]

use domain::{InvariantOwnerType, InvariantViolation, domain_invariants};

#[domain_invariants(entity)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn __DOMAIN_INVARIANT_REFERENCE_VALID(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

fn main() {}
