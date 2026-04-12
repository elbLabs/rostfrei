use rostfrei_domain::{BoundedContext, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "normalize", label = "Normalize")]
    fn normalize(self) -> Self;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

fn main() {}
