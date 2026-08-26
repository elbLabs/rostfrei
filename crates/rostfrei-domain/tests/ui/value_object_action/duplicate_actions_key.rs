use domain::ValueObject;

struct Context;
trait Actions {}

#[derive(ValueObject)]
#[domain(
    id = "name",
    label = "Name",
    owner = Context,
    actions = [Actions],
    actions = [Actions]
)]
struct Name(String);

fn main() {}
