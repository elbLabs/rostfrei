use rostfrei_domain::domain_actions;

trait Base {}

#[domain_actions(entity)]
trait Actions: Base {
    #[action(id = "rename", label = "Rename")]
    fn rename(&self);
}

fn main() {}
