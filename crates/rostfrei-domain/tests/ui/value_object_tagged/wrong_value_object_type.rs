use domain::{BoundedContext, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

struct Plain;

#[derive(ValueObject)]
#[domain(id = "invalid", label = "Invalid", owner = Context)]
enum Invalid {
    Value(#[domain(value_object)] Plain),
}

fn main() {}
