use rostfrei_domain::domain_actions;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute((input,): (u8,));
}

fn main() {}
