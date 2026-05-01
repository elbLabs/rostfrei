#[domain_invariant(id = "duplicate-invariant", label = "Duplicate invariant")]
pub trait DuplicateInvariant {
    fn valid(&self) -> bool;
}
