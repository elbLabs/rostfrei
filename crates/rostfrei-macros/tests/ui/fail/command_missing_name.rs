use rostfrei_macros::CommandDefinition;

struct Account;

#[derive(CommandDefinition)]
#[rostfrei(version = 1, aggregate = Account)]
struct OpenAccount;

fn main() {}
