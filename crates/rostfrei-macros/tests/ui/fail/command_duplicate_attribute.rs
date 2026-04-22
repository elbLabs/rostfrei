use rostfrei_macros::CommandDefinition;

struct Account;

#[derive(CommandDefinition)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account, name = "account.create")]
struct OpenAccount;

fn main() {}
