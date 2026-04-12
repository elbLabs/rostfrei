use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "Not-Kebab", label = "Invalid")]
    fn invalid(&self);
}

fn main() {}
