use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "first", label = "First")]
    #[action(id = "second", label = "Second")]
    fn duplicate(&self);
}

fn main() {}
