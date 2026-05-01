#[domain_decision(id = "missing", label = "Missing")]
pub trait MissingDecision {
    fn decide(&self);
}
