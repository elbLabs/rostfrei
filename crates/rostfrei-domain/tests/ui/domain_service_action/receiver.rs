use domain::domain_actions;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute(&self);
}

fn main() {}
