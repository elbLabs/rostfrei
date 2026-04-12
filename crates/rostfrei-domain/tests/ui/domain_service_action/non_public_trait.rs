use rostfrei_domain::domain_actions;

#[domain_actions(domain_service)]
trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute();
}

fn main() {}
