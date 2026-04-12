use rostfrei_domain::domain_actions;

pub struct Root;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &Root);
}

fn main() {}
