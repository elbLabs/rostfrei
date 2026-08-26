use rostfrei_macros::Command;

struct Account;

#[derive(Command)]
#[rostfrei(name = "account.open", version = 0, aggregate = Account)]
struct OpenAccount;

fn main() {}
