use rostfrei_macros::CommandDefinition;

struct Account;

#[derive(CommandDefinition)]
#[rostfrei(name = "account.open", version = 0, aggregate = Account)]
struct OpenAccount;

fn main() {}
