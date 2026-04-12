use rostfrei_domain::domain_actions;

pub struct Root;
pub struct Input;

#[domain_actions(aggregate)]
pub trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Root, input: Input, other: Input);
}

fn main() {}
