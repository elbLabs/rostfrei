struct MoneyDeposited;

#[rostfrei::domain_actions(aggregate(instance = AccountActions))]
pub trait AccountActionContract {
    #[action(id = "deposit", label = "Deposit", raises = [MoneyDeposited])]
    fn deposit(self: &mut Self);
}

fn main() {}
