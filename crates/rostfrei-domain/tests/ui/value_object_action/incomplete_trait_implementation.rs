use domain::{BoundedContext, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "new", label = "New")]
    fn new(input: String) -> Self;

    #[action(id = "normalize", label = "Normalize")]
    fn normalize(self) -> Self;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn new(input: String) -> Self {
        Self(input)
    }
}

fn main() {}
