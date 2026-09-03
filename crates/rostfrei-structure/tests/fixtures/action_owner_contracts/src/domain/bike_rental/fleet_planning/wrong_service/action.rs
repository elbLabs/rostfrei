#[domain_action(id = "wrong-service", label = "Wrong service")]
pub trait WrongServiceAction {
    fn execute(&mut self);
}
