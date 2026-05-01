#[domain_action(id = "wrong-trait", label = "Wrong trait")]
pub trait WrongTraitAction {
    fn execute(&mut self);
}
