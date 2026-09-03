#[domain_query(id = "invalid-query", label = "Invalid query")]
pub trait InvalidQuery {
    fn query(&self) -> bool;
}
