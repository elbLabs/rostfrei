#[domain_action(id = "duplicate-impl", label = "Duplicate impl")]
pub trait DuplicateImplAction {
    fn execute(&mut self);
}
