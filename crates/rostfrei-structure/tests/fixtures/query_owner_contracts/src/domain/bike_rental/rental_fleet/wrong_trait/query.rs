#[domain_query(id = "wrong-trait", label = "Wrong trait")]
pub trait WrongTraitQuery {
    fn query(&self);
}
