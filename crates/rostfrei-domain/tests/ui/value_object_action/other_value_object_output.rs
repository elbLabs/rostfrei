use rostfrei_domain::{BoundedContext, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "other-name", label = "Other name", owner = Context)]
struct OtherName(String);

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "convert", label = "Convert")]
    fn convert(self) -> OtherName;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn convert(self) -> OtherName {
        OtherName(self.0)
    }
}

fn main() {}
