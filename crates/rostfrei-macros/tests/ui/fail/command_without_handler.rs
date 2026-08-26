use rostfrei_macros::Command;
use zs_core::Aggregate;

struct Account;

impl Aggregate for Account {
    type Event = ();

    const AGGREGATE_TYPE: &'static str = "account";

    fn initial() -> Self {
        Self
    }

    fn apply(&mut self, (): &Self::Event) {}
}

#[derive(Command)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account)]
struct OpenAccount;

fn main() {}
