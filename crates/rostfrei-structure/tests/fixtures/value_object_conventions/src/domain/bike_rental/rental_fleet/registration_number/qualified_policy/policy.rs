#[domain_policy(id = "qualified-policy", label = "Qualified policy")]
pub trait QualifiedPolicy {
    fn evaluate(&self) -> bool;
}
