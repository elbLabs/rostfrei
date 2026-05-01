#[domain_invariant(id = "wrong-entity", label = "Wrong entity")]
pub trait WrongEntityInvariant {
    fn validate(&self);
}
