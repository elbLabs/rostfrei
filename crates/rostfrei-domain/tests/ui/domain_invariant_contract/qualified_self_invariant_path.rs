#![allow(unused, non_snake_case)]

use domain::ValueObject;

struct Context;
struct Contracts;
trait ContractSet {
    type Invariants;
}
impl ContractSet for Contracts {
    type Invariants = ();
}

#[derive(ValueObject)]
#[domain(
    id = "value",
    label = "Value",
    owner = Context,
    invariants = [<Contracts as ContractSet>::Invariants]
)]
struct Value(u8);

fn main() {}
