use rostfrei_macros::Command;
use zs_core::{Aggregate, StreamId};

struct Account;

impl Aggregate for Account {
    type State = Self;
    type Event = ();

    const AGGREGATE_TYPE: &'static str = "account";

    fn initial(_stream_id: &StreamId) -> Self::State {
        Self
    }

    fn apply(_state: &mut Self::State, (): &Self::Event) {}
}

#[derive(Command)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account)]
struct OpenAccount;

fn main() {}
