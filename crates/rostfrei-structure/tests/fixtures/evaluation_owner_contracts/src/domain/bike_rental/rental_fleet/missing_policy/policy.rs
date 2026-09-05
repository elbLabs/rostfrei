#[domain_policy(id = "missing", label = "Missing")]
pub trait MissingPolicy {
    fn evaluate(&self);
}
