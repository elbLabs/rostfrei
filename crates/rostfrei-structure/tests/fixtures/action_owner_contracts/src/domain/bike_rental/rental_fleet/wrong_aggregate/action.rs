#[domain_action(id = "wrong-aggregate", label = "Wrong aggregate")]
pub trait WrongAggregateAction {
    fn execute(&mut self);
}
