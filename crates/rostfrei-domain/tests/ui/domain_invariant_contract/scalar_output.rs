#![allow(unused, non_snake_case)]

use domain::{InvariantOwnerType, domain_invariants};

#[domain_invariants(entity)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(candidate: &<Self as InvariantOwnerType>::Candidate) -> bool;
}

fn main() {}
