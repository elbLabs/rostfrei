#[domain_decision(id = "glob", label = "Glob")]
pub trait GlobDecision {
    fn decide(&self);
}
