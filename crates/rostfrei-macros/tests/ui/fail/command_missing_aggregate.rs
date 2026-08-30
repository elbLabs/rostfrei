use rostfrei_macros::CommandDefinition;

#[derive(CommandDefinition)]
#[rostfrei(name = "account.open", version = 1)]
struct OpenAccount;

fn main() {}
