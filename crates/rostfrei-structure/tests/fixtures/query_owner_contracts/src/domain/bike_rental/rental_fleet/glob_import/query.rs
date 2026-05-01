#[domain_query(id = "glob-import", label = "Glob import")]
pub trait GlobImportQuery {
    fn query(&self);
}
