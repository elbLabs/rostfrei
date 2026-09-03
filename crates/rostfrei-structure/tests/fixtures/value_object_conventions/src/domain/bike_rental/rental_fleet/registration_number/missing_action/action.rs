#[domain_action(id = "missing-action", label = "Missing action")]
pub trait MissingAction {
    fn execute(&mut self);
}
