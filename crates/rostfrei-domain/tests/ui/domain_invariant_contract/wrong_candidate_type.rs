#![allow(unused, non_snake_case)]

use domain::{InvariantViolation, domain_invariants};

#[domain_invariants(entity)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(candidate: &Self) -> Option<InvariantViolation>;
}

fn main() {}
