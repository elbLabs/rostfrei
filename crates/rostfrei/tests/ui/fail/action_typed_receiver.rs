#[rostfrei::domain_actions(aggregate(instance = AccountActions))]
pub trait AccountActionContract {
    #[action(id = "deposit", label = "Deposit")]
    fn deposit(self: &mut Self);
}

fn main() {}
