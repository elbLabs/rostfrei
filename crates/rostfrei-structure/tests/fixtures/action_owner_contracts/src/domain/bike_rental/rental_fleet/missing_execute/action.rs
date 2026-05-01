#[domain_action(id = "missing-execute", label = "Missing execute")]
pub trait MissingExecuteAction {
    fn execute(&mut self);
}
