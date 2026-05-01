#[domain_decision(id = "wrong-trait", label = "Wrong trait")]
pub trait WrongDecisionTrait {
    fn decide(&self);
}
