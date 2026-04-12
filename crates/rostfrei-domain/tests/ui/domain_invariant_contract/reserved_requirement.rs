#![allow(unused, non_snake_case)]

use domain::domain_invariants;

#[domain_invariants(entity)]
trait Invariants {
    const __DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE: ();
}

fn main() {}
