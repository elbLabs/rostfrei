use rostfrei::{AggregateInstance, Apply, CommandHandler, Initialize};
use serde::{Deserialize, Serialize};

#[derive(rostfrei::BoundedContext)]
#[rostfrei(id = "banking", label = "Banking")]
struct Banking;

#[derive(rostfrei::DomainIdentity)]
struct AccountId(String);

#[derive(rostfrei::Entity)]
#[rostfrei(id = "account", label = "Account")]
struct Account {
    id: AccountId,
    balance: i64,
}

impl rostfrei::EntityDefinition for Account {
    type Owner = AccountAggregate;
    type Identity = AccountId;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[rostfrei::domain_action(id = "reset", label = "Reset")]
trait ResetAccountAction {
    fn reset(&mut self);
}

impl ResetAccountAction for Account {
    fn reset(&mut self) {
        self.balance = 0;
    }
}

#[derive(rostfrei::ValueObject)]
#[rostfrei(id = "amount", label = "Amount")]
struct Amount(i64);

#[derive(Serialize, Deserialize, rostfrei::DomainEvent)]
#[rostfrei(id = "money-deposited", label = "Money deposited")]
struct MoneyDeposited {
    amount: i64,
}

mod aggregate_actions {
    use super::{AccountAggregate, AggregateInstance, MoneyDeposited};

    #[rostfrei::domain_action(id = "deposit", label = "Deposit")]
    pub trait DepositAction {
        fn deposit(&mut self, input: i64);
    }

    impl DepositAction for AggregateInstance<AccountAggregate> {
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
        use aggregate_actions::DepositAction as _;
        aggregate.deposit(command.0);
        Ok(())
    }
}

#[rostfrei::domain_action_test(<Account as ResetAccountAction>::DESCRIPTOR)]
fn facade_domain_test_support_items_are_available() {}

#[rostfrei::domain_action_test(
    <AggregateInstance<AccountAggregate> as aggregate_actions::DepositAction>::DESCRIPTOR
)]
fn facade_executable_aggregate_action_is_the_test_subject() {}

fn main() {
    let _executor = rostfrei::Executor::new(rostfrei::InMemoryEventStore::new());
    let _reset: fn(&mut Account) = <Account as ResetAccountAction>::reset;
}
