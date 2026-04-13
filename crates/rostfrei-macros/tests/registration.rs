use rostfrei_macros::{Command, Module};
use zs_core::{Aggregate, AggregateInstance, CommandHandler, StreamId};
use zs_registry::{CommandDefinition, DomainRegistry};

struct Account {
    balance: i64,
}

enum AccountEvent {
    Deposited(i64),
}

impl Aggregate for Account {
    type State = Self;
    type Event = AccountEvent;

    const AGGREGATE_TYPE: &'static str = "account";

    fn initial(_stream_id: &StreamId) -> Self::State {
        Self { balance: 0 }
    }

    fn apply(state: &mut Self::State, event: &Self::Event) {
        let AccountEvent::Deposited(amount) = event;
        state.balance += amount;
    }
}

#[derive(Command)]
#[rostfrei(name = "account.deposit", version = 1, aggregate = Account)]
struct Deposit {
    amount: i64,
}

impl CommandHandler<Deposit> for Account {
    type Rejection = ();

    fn handle(
        command: &Deposit,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        aggregate.raise(AccountEvent::Deposited(command.amount));
        Ok(())
    }
}

#[derive(Module)]
#[rostfrei(name = "accounts", commands(Deposit))]
struct Accounts;

#[test]
fn derived_domain_types_register_and_query_without_state_trait_requirements() {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Accounts>().unwrap();

    let command = registry
        .command("account.deposit", 1)
        .expect("derived command should be registered");

    assert_eq!(command.command_name, "account.deposit");
    assert_eq!(command.schema_version, 1);
    assert_eq!(command.aggregate_type, "account");
    assert_eq!(
        <Deposit as CommandDefinition>::descriptor().command_name,
        "account.deposit"
    );
}
