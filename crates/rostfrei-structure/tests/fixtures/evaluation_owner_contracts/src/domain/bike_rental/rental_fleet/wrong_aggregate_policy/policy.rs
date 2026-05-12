#[domain_policy(id = "wrong-owner", label = "Wrong owner")]
pub trait WrongAggregatePolicy {
    fn evaluate(&self);
}
