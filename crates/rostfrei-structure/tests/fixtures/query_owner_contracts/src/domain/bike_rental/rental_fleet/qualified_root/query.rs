#[domain_query(id = "qualified-root", label = "Qualified root")]
pub trait QualifiedRootQuery {
    fn query(&self);
}
