use domain::ValueObject;

struct Context;
trait Actions {}

#[derive(ValueObject)]
#[domain(
    id = "name",
    label = "Name",
    owner = Context,
    actions = [Actions, Actions]
)]
struct Name(String);

fn main() {}
