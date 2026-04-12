use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "duplicate", id = "other", label = "Duplicate")]
    fn duplicate(&self);
}

fn main() {}
