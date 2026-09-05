#[domain_policy(id = "wrong-trait", label = "Wrong trait")]
pub trait WrongPolicyTrait {
    fn evaluate(&self);
}
