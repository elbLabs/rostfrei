#![allow(unused, non_snake_case)]

use domain::domain_invariants;

#[domain_invariants(entity)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(&self) -> Option<domain::InvariantViolation>;
}

fn main() {}
