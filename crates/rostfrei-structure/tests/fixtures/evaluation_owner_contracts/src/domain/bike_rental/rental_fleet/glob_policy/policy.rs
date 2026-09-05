#[domain_policy(id = "glob", label = "Glob")]
pub trait GlobPolicy {
    fn evaluate(&self);
}
