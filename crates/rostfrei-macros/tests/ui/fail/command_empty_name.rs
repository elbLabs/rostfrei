use rostfrei_macros::Command;

struct Account;

#[derive(Command)]
#[rostfrei(name = "", version = 1, aggregate = Account)]
struct OpenAccount;

fn main() {}
