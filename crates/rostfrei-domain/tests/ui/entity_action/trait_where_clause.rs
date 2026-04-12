use domain::domain_actions;

#[domain_actions(entity)]
trait Actions
where
    Self: Send,
{
    #[action(id = "rename", label = "Rename")]
    fn rename(&self);
}

fn main() {}
