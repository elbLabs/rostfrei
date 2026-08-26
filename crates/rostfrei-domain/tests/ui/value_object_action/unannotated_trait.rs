use domain::{BoundedContext, ValueObject};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

trait Actions {
    fn normalize(self) -> Self;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn normalize(self) -> Self {
        self
    }
}

fn main() {}
