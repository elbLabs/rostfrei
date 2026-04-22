use rostfrei_macros::CommandDefinition;

struct Account;

#[derive(CommandDefinition)]
#[rostfrei(name = "", version = 1, aggregate = Account)]
struct OpenAccount;

fn main() {}
