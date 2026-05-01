#[domain_action(id = "wrong-entity", label = "Wrong entity")]
pub trait WrongEntityAction {
    fn execute(&mut self);
}
