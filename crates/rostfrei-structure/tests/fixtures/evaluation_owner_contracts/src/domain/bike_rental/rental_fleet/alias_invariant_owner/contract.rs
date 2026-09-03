#[domain_invariant(id = "alias-owner", label = "Alias owner")]
pub trait AliasInvariantOwner {
    fn validate(&self);
}
