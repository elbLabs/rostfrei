#[rostfrei::domain_actions(aggregate(instance = AccountActions))]
pub trait AccountActionContract {
    #[action(id = "deposit", label = "Deposit")]
    fn deposit(&'static mut self);
}

fn main() {}
