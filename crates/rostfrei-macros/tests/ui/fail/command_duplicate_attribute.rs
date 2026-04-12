use rostfrei_macros::Command;

struct Account;

#[derive(Command)]
#[rostfrei(name = "account.open", version = 1, aggregate = Account, name = "account.create")]
struct OpenAccount;

fn main() {}
