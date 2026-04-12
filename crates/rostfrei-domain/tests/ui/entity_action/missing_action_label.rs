use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "missing-label")]
    fn missing(&self);
}

fn main() {}
