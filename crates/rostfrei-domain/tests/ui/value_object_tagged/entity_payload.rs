use rostfrei_domain::{BoundedContext, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

struct Other;

#[derive(ValueObject)]
#[domain(id = "invalid", label = "Invalid", owner = Context)]
enum Invalid {
    Entity(#[domain(entity)] Other),
}

fn main() {}
