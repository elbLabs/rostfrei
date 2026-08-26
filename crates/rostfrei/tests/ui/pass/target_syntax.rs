use rostfrei::{Apply, CommandHandler, DecisionContext, Initialize};
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
    balance: i64,
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

#[derive(rostfrei::Aggregate)]
#[rostfrei(
    id = "account",
    label = "Account",
    context = Banking,
    root = Account,
    events = [MoneyDeposited]
)]
struct AccountAggregate;

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
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        context.record(MoneyDeposited { amount: command.0 });
        Ok(())
    }
}

#[rostfrei::domain_action_test(<Account as AccountActions>::RESET)]
fn facade_domain_test_support_items_are_available() {}

fn main() {
    let _executor = rostfrei::Executor::new(rostfrei::InMemoryEventStore::new());
}
