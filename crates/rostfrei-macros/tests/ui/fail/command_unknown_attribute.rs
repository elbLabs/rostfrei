use rostfrei_macros::Command;

struct Account;

#[derive(Command)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account, unexpected = true)]
struct OpenAccount;

fn main() {}
