use rostfrei_macros::Module;

struct UndeclaredCommand;

#[derive(Module)]
#[rostfrei(name = "accounts", commands(UndeclaredCommand))]
struct Accounts;

fn main() {}
