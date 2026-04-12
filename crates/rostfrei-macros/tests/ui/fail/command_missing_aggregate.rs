use rostfrei_macros::Command;

#[derive(Command)]
#[rostfrei(name = "account.open", version = 1)]
struct OpenAccount;

fn main() {}
