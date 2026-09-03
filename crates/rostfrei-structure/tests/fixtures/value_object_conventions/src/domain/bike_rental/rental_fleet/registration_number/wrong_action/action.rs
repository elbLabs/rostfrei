#[domain_action(id = "wrong-action", label = "Wrong action")]
pub trait WrongAction {
    fn execute(&mut self);
}
