#[domain_invariant(id = "qualified-owner", label = "Qualified owner")]
pub trait QualifiedInvariantOwner {
    fn validate(&self);
}
