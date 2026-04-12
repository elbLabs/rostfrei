use rostfrei_domain::{BoundedContext, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "normalize", label = "Normalize")]
    fn normalize(self) -> Option<Self>;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn normalize(self) -> Option<Self> {
        Some(self)
    }
}

fn main() {}
