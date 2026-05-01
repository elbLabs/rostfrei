#[domain_decision(id = "wrong-owner", label = "Wrong owner")]
pub trait WrongAggregateDecision {
    fn decide(&self);
}
