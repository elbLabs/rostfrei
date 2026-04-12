use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action]
    fn malformed(&self);
}

fn main() {}
