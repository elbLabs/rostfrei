#[domain_invariant(id = "duplicate", label = "Duplicate")]
pub trait DuplicateInvariant {
    fn validate(&self);
}
