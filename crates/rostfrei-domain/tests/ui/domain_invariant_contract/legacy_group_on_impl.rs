#![allow(unused, non_snake_case)]

use domain::domain_invariants;

struct Owner;

#[domain_invariants(group = Invariants)]
impl Owner {}

fn main() {}
