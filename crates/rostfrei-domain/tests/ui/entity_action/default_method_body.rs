use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "rename", label = "Rename")]
    fn rename(&self) {}
}

fn main() {}
