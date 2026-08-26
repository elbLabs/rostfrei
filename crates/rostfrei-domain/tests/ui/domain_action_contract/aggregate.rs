use domain::domain_actions;

pub struct Root;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Root);
}

fn main() {}
