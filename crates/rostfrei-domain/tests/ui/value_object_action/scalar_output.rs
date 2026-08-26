use domain::{BoundedContext, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "length", label = "Length")]
    fn length(self) -> usize;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn length(self) -> usize {
        self.0.len()
    }
}

fn main() {}
