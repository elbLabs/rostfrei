#[domain_query(id = "duplicate-impl", label = "Duplicate impl")]
pub trait DuplicateImplQuery {
    fn query(&self);
}
