use rostfrei_macros::Command;

struct Account;

#[derive(Command)]
#[rostfrei(name = "account.open", aggregate = Account)]
struct OpenAccount;

fn main() {}
