use rostfrei_domain::{BoundedContext, DomainError, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[derive(ValueObject)]
#[domain(id = "other-name", label = "Other name", owner = Context)]
struct OtherName(String);

#[derive(DomainError)]
#[domain(
    id = "wrong",
    label = "Wrong",
    owner = OtherName,
    code = "WRONG",
    message = "Wrong owner."
)]
struct WrongError;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "rename", label = "Rename")]
    fn rename(self, input: String) -> Result<Self, WrongError>;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

impl Actions for Name {
    fn rename(self, input: String) -> Result<Self, WrongError> {
        let _ = self;
        Ok(Self(input))
    }
}

fn main() {}
