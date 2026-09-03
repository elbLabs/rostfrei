use super::DecisionId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionDescriptor {
    pub id: DecisionId,
    pub label: &'static str,
}
