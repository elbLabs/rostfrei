use super::PolicyId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyDescriptor {
    pub id: PolicyId,
    pub label: &'static str,
}
