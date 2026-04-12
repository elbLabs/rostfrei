use rostfrei_macros::{Command, Module};
use zs_core::{Aggregate, CommandHandler, DecisionContext};
use zs_registry::{CommandDefinition, DomainRegistry};

struct Account {
    balance: i64,
}

enum AccountEvent {
    Deposited(i64),
}

impl Aggregate for Account {
    type Event = AccountEvent;

    const AGGREGATE_TYPE: &'static str = "account";

    fn initial() -> Self {
        Self { balance: 0 }
    }

    fn apply(&mut self, event: &Self::Event) {
        let AccountEvent::Deposited(amount) = event;
        self.balance += amount;
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
        context: &mut DecisionContext<'_, Self>,
    ) -> Result<(), Self::Rejection> {
        context.record(AccountEvent::Deposited(command.amount));
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
