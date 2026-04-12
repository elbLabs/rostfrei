use rostfrei_domain::Aggregate;

struct Context;
struct Root;
trait Actions {}

#[derive(Aggregate)]
#[domain(
    id = "owner",
    label = "Owner",
    context = Context,
    root = Root,
    actions = Actions
)]
struct Owner;

fn main() {}
