use super::{
    DecisionId, DecisionImplementationDescriptor, DecisionOutcomeDescriptor,
    DecisionParameterDescriptor,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionDescriptor {
    pub id: DecisionId,
    pub label: &'static str,
    pub parameters: &'static [DecisionParameterDescriptor],
    pub outcomes: &'static [DecisionOutcomeDescriptor],
    pub implementation: DecisionImplementationDescriptor,
}
