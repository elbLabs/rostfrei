#[domain_action(id = "qualified-owner", label = "Qualified owner")]
pub trait QualifiedOwnerAction {
    fn execute(&mut self);
}
