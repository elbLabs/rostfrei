#![allow(unused, non_snake_case)]

use domain::domain_invariants;

#[domain_invariants(entity)]
trait Invariants {
    const __DOMAIN_INVARIANTS: ();
}

fn main() {}
