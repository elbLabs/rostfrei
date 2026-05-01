#[domain_query(id = "missing-execute", label = "Missing execute")]
pub trait MissingExecuteQuery {
    fn query(&self);
}
