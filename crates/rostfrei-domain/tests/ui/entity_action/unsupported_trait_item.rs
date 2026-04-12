use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    const ENABLED: bool;
}

fn main() {}
