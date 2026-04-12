#![allow(unused, non_snake_case)]

use rostfrei_domain::ValueObject;

struct Context;
trait Invariants<T> {}

#[derive(ValueObject)]
#[domain(
    id = "value",
    label = "Value",
    owner = Context,
    invariants = [Invariants<u8>]
)]
struct Value(u8);

fn main() {}
