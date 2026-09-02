#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionOutcomeDescriptor {
    pub local_id: &'static str,
    pub label: &'static str,
}
