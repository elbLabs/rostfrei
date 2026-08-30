use rostfrei_macros::CommandDefinition;

struct Account;

#[derive(CommandDefinition)]
#[rostfrei(name = "account.open", aggregate = Account)]
struct OpenAccount;

fn main() {}
