#![allow(dead_code)]

use rostfrei_domain::{BoundedContext, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "supported", label = "Supported", owner = Context)]
enum Supported {
    Unit,
    EmptyTuple(),
    EmptyStruct {},
    Tuple(u8, Option<Vec<String>>),
    Struct { enabled: bool },
}

fn main() {}
