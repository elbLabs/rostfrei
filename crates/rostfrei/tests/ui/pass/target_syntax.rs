use rostfrei::{AggregateInstance, Apply, CommandHandler, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "banking", label = "Banking")]
struct Banking;

#[derive(rostfrei::DomainIdentity)]
#[rostfrei(owner = Account)]
struct AccountId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "account", label = "Account")]
struct Account {
    #[rostfrei(identity)]
    id: AccountId,
    balance: i64,
}

impl rostfrei::EntityDefinition for Account {
    type Owner = AccountAggregate;
    type Identity = AccountId;
}

#[rostfrei::domain_actions(entity)]
trait AccountActions {
    #[action(id = "reset", label = "Reset")]
    fn reset(&mut self);
}

impl AccountActions for Account {
    fn reset(&mut self) {
        self.balance = 0;
    }
}

#[rostfrei::domain_actions(value_object)]
trait AmountActions {
    #[action(id = "new", label = "New")]
    fn new(input: i64) -> Self;
}

#[derive(rostfrei::ValueObject)]
#[rostfrei(id = "amount", label = "Amount", owner = Banking, actions = [AmountActions])]
struct Amount(i64);

impl AmountActions for Amount {
    fn new(input: i64) -> Self {
        Self(input)
    }
}

#[derive(Serialize, Deserialize, rostfrei::DomainEvent)]
#[rostfrei(id = "money-deposited", label = "Money deposited")]
struct MoneyDeposited {
    amount: i64,
}

mod aggregate_actions {
    use super::{AccountAggregate, AggregateInstance, MoneyDeposited};

    #[rostfrei::domain_actions(aggregate(instance = AccountAggregateActions))]
    pub trait AccountAggregateActionContract {
        #[action(
            id = "deposit",
            label = "Deposit",
            raises = [MoneyDeposited]
        )]
        fn deposit(&mut self, input: i64);
    }

    impl AccountAggregateActions for AggregateInstance<AccountAggregate> {
        fn deposit(&mut self, input: i64) {
            self.raise(MoneyDeposited { amount: input });
        }
    }
}

#[derive(rostfrei::AggregateEvents)]
enum AccountEvents {
    MoneyDeposited(MoneyDeposited),
}

#[derive(rostfrei::Aggregate)]
#[rostfrei(id = "account", label = "Account")]
struct AccountAggregate;

impl rostfrei::AggregateDefinition for AccountAggregate {
    type Context = Banking;
    type Root = Account;
    type Event = AccountEvents;
}

use aggregate_actions::AccountAggregateActions as _;

impl Initialize<AccountAggregate> for Account {
    fn initialize(stream_id: &rostfrei::StreamId) -> Self {
        Self {
            id: AccountId(stream_id.aggregate_id().as_str().to_owned()),
            balance: 0,
        }
    }
}

impl Apply<MoneyDeposited> for Account {
    fn apply(&mut self, event: &MoneyDeposited) {
        self.balance += event.amount;
    }
}

struct Deposit(i64);

impl CommandHandler<Deposit> for AccountAggregate {
    type Rejection = ();

    fn handle(
        command: &Deposit,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.deposit(command.0);
        Ok(())
    }
}

#[rostfrei::domain_action_test(<Account as AccountActions>::RESET)]
fn facade_domain_test_support_items_are_available() {}

#[rostfrei::domain_action_test(
    <AccountAggregate as aggregate_actions::AccountAggregateActionContract>::DEPOSIT
)]
fn facade_executable_aggregate_action_is_the_test_subject() {}

fn main() {
    let _executor = rostfrei::Executor::new(rostfrei::InMemoryEventStore::new());
}
