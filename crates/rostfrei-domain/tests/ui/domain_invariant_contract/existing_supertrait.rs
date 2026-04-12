#![allow(unused, non_snake_case)]

use domain::{InvariantOwnerType, InvariantViolation, domain_invariants};

trait Existing {}

#[domain_invariants(entity)]
trait Invariants: Existing {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

fn main() {}
