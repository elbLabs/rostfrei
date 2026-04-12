#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "context", label = "Context")]
struct Context;

struct Root;
struct Event;

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "aggregate",
    label = "Aggregate",
    context = Context,
    root = Root,
    events = Event
)]
struct Aggregate;

fn main() {}
