struct Event;

#[derive(rostfrei::AggregateEvents)]
enum Events {
    Event { event: Event },
}

fn main() {}
