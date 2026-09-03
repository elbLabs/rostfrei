#[domain_decision(id = "qualified-decision", label = "Qualified decision")]
pub trait QualifiedDecision {
    fn decide(&self) -> bool;
}
