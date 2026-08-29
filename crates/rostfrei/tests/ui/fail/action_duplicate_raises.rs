struct MoneyDeposited;

#[rostfrei::domain_actions(aggregate(instance = AccountActions))]
pub trait AccountActionContract {
    #[action(
        id = "deposit",
        label = "Deposit",
        raises = [MoneyDeposited, MoneyDeposited]
    )]
    fn deposit(&mut self);
}

fn main() {}
