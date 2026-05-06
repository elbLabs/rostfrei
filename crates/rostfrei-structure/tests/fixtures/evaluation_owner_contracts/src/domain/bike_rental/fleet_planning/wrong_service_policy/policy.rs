#[domain_policy(id = "wrong-service-policy", label = "Wrong service policy")]
pub trait WrongServicePolicy {
    fn evaluate();
}
