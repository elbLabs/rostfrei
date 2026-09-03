#[derive(rostfrei::DomainEvent)]
#[rostfrei(id = "event", label = "Event", schema_version = 0)]
struct Event;

fn main() {}
rostfrei::install_macro_support!();
