use super::{DecisionId, DecisionImplementationDescriptor, DecisionOutcomeDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionDescriptor {
    pub id: DecisionId,
    pub label: &'static str,
    pub outcomes: &'static [DecisionOutcomeDescriptor],
    pub implementation: DecisionImplementationDescriptor,
}
