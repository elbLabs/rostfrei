#![allow(unused, non_snake_case)]

use domain::ValueObject;

struct Context;
trait Invariants {}

#[derive(ValueObject)]
#[domain(
    id = "value",
    label = "Value",
    owner = Context,
    invariants = [Invariants],
    invariants = [Invariants]
)]
struct Value(u8);

fn main() {}
