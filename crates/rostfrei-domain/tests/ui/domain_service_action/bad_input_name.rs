use rostfrei_domain::domain_actions;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute(command: u8);
}

fn main() {}
