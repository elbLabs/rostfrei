use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    fn rename(&self);
}

fn main() {}
