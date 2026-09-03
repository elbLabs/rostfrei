use rostfrei_macros::{CommandDefinition, Module, QueryDefinition};
use zs_core::{Aggregate, AggregateInstance, CommandHandler, StreamId};
use zs_registry::{CommandDefinition, DomainRegistry, QueryDefinition};

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
        state.balance = state.balance.saturating_add(*amount);
    }
}

#[derive(CommandDefinition)]
#[rostfrei(name = "account-deposit", version = 1, aggregate = Account)]
struct Deposit {
    amount: i64,
}

#[derive(QueryDefinition)]
#[rostfrei(
    context = "accounts",
    name = "account-balance",
    version = 1,
    response = i64
)]
#[allow(dead_code)]
struct AccountBalance {
    account_id: String,
}

impl CommandHandler<Deposit> for Account {
    type Rejection = ();

    fn handle(
        command: &Deposit,
        aggregate: &mut AggregateInstance<Self>,
    ) -> Result<(), Self::Rejection> {
        if aggregate
            .state()
            .balance
            .checked_add(command.amount)
            .is_none()
        {
            return Err(());
        }
        aggregate.raise(AccountEvent::Deposited(command.amount));
        Ok(())
    }
}

#[derive(Module)]
#[rostfrei(name = "accounts", commands(Deposit), queries(AccountBalance))]
struct Accounts;

fn assert_registered_command(command: &zs_registry::CommandDescriptor) {
    assert_eq!(command.command_name, "account-deposit");
    assert_eq!(command.schema_version, 1);
    assert_eq!(command.aggregate_type, "account");
    assert_eq!(
        <Deposit as CommandDefinition>::descriptor().command_name,
        "account-deposit"
    );
}

fn assert_registered_query(query: &zs_registry::QueryDescriptor) {
    assert_eq!(query.rust_response_type, "i64");
    assert_eq!(AccountBalance::QUERY_NAME, "account-balance");
}

#[test]
fn derived_domain_types_register_and_query_without_state_trait_requirements()
-> Result<(), Box<dyn std::error::Error>> {
    let mut registry = DomainRegistry::new();
    registry.register_module::<Accounts>()?;

    let command = registry
        .command("account", "account-deposit", 1)
        .ok_or_else(|| std::io::Error::other("derived command should be registered"))?;

    assert_registered_command(command);
    let query = registry
        .query("accounts", "account-balance", 1)
        .ok_or_else(|| std::io::Error::other("derived query should be registered"))?;
    assert_registered_query(query);
    Ok(())
}
