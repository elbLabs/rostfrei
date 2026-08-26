use rostfrei_domain::domain_actions;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change();
}

fn main() {}
