use rostfrei_domain::{BoundedContext, DomainError, ValueObject, domain_actions};

#[derive(BoundedContext)]
#[domain(id = "context", label = "Context")]
struct Context;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "new", label = "New")]
    fn new(input: String) -> Self;

    #[action(id = "normalize", label = "Normalize")]
    fn normalize(self) -> Self;

    #[action(id = "rename", label = "Rename")]
    fn rename(self, input: String) -> Result<Self, NameError>;
}

#[derive(ValueObject)]
#[domain(id = "name", label = "Name", owner = Context, actions = [Actions])]
struct Name(String);

#[derive(DomainError)]
#[domain(
    id = "invalid-name",
    label = "Invalid name",
    owner = Name,
    code = "INVALID_NAME",
    message = "The name is invalid."
)]
struct NameError;

impl Actions for Name {
    fn new(input: String) -> Self {
        Self(input)
    }

    fn normalize(self) -> Self {
        self
    }

    fn rename(self, input: String) -> Result<Self, NameError> {
        let _ = self;
        Ok(Self(input))
    }
}

fn main() {}
