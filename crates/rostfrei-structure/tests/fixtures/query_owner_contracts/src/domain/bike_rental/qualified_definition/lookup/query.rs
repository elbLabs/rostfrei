#[domain_query(id = "lookup", label = "Lookup")]
pub trait LookupQuery {
    fn lookup(&self);
}
