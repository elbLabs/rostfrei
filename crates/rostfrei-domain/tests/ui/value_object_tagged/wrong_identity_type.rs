use rostfrei_domain::{BoundedContext, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

struct Plain;

#[derive(ValueObject)]
#[domain(id = "invalid", label = "Invalid", owner = Context)]
enum Invalid {
    Identity(#[domain(identity)] Plain),
}

fn main() {}
