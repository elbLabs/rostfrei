#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyOutcomeDescriptor {
    pub local_id: &'static str,
    pub label: &'static str,
}
