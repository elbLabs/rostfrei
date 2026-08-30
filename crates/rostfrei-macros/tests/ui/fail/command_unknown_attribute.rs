use rostfrei_macros::CommandDefinition;

struct Account;

#[derive(CommandDefinition)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account, unexpected = true)]
struct OpenAccount;

fn main() {}
