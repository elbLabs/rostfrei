use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "change", label = "Rename")]
    fn rename(&self);

    #[action(id = "change", label = "Archive")]
    fn archive(&self);
}

fn main() {}
