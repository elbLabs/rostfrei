struct MoneyDeposited;

#[rostfrei::domain_actions(aggregate(instance = AccountActions))]
pub trait AccountActionContract {
    #[action(id = "deposit", label = "Deposit")]
    fn deposit(&mut self, input: i64) -> MoneyDeposited;
}

fn main() {}
