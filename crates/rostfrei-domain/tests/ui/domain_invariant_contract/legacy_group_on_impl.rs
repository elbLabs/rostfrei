#![allow(unused, non_snake_case)]

use rostfrei_domain::domain_invariants;

struct Owner;

#[domain_invariants(group = Invariants)]
impl Owner {}

fn main() {}
