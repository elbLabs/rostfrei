#![allow(unused, non_snake_case)]

use domain::{InvariantOwnerType, InvariantViolation, domain_invariants};

#[domain_invariants(entity)]
trait Invariants
where
    Self: Sized,
{
    #[invariant(id = "valid", label = "Valid")]
    fn valid(
        candidate: &<Self as InvariantOwnerType>::Candidate,
    ) -> Option<InvariantViolation>;
}

fn main() {}
