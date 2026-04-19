struct MoneyDeposited;

#[rostfrei::domain_actions(aggregate(instance = AccountActions))]
pub trait AccountActionContract {
    #[action(id = "deposit", label = "Deposit", raises = [])]
    fn deposit(&mut self);
}

fn main() {}
