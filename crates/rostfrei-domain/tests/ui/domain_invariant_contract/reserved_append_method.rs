#![allow(unused, non_snake_case)]

use domain::domain_invariants;

#[domain_invariants(entity)]
trait Invariants {
    fn __DOMAIN_INVARIANTS_APPEND_VIOLATIONS();
}

fn main() {}
