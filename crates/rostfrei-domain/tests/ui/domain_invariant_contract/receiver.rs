#![allow(unused, non_snake_case)]

use rostfrei_domain::domain_invariants;

#[domain_invariants(entity)]
trait Invariants {
    #[invariant(id = "valid", label = "Valid")]
    fn valid(&self) -> Option<rostfrei_domain::InvariantViolation>;
}

fn main() {}
