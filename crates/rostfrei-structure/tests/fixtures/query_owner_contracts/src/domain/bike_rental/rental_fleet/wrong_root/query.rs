#[domain_query(id = "wrong-root", label = "Wrong root")]
pub trait WrongRootQuery {
    fn query(&self);
}
