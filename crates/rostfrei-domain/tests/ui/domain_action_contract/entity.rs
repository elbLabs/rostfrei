use domain::domain_actions;

#[domain_actions(entity)]
trait EntityActions {
    #[action(id = "inspect", label = "Inspect")]
    fn inspect(&self);
}

fn main() {}
