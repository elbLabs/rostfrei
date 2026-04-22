use rostfrei::{AggregateInstance, Apply, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "banking", label = "Banking")]
struct Banking;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = Account)]
struct AccountId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "account", label = "Account", owner = AccountAggregate)]
struct Account {
    #[rostfrei(identity)]
    id: AccountId,
}

#[derive(Serialize, Deserialize, rostfrei::DomainEvent)]
#[rostfrei(id = "account-opened", label = "Account opened")]
struct AccountOpened;

#[derive(rostfrei::Command)]
#[rostfrei(id = "open-account", label = "Open account", owner = AccountAggregate)]
struct OpenAccount;

mod actions {
    use super::{AccountAggregate, AccountOpened, AggregateInstance, OpenAccount};

    #[rostfrei::domain_actions(aggregate(instance = AccountActions))]
    pub trait AccountActionContract {
        #[action(
            id = "open-account",
            label = "Open account",
            raises = [AccountOpened]
        )]
        fn open_account(&mut self, input: OpenAccount);
    }

    impl AccountActions for AggregateInstance<AccountAggregate> {
        fn open_account(&mut self, _input: OpenAccount) {
            self.raise(AccountOpened);
        }
    }
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "account",
    label = "Account",
    context = Banking,
    root = Account,
    actions = [actions::AccountActionContract],
    events = [AccountOpened]
)]
struct AccountAggregate;

impl Initialize<AccountAggregate> for Account {
    fn initialize(_stream_id: &rostfrei::StreamId) -> Self {
        Self {
            id: AccountId(String::new()),
        }
    }
}

impl Apply<AccountOpened> for Account {
    fn apply(&mut self, _event: &AccountOpened) {}
}

fn main() {}
