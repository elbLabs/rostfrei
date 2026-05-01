struct Event;

#[derive(rostfrei::AggregateEvents)]
enum Events {
    Event { event: Event },
}

fn main() {}
rostfrei::install_macro_support!();
