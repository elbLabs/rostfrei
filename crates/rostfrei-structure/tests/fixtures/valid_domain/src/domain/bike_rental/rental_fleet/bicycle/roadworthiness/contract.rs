#[domain_invariant(id = "roadworthy", label = "Bicycle is roadworthy")]
pub trait Roadworthiness {
    fn is_roadworthy(&self) -> bool;
}
