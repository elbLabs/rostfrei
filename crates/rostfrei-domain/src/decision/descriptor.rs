use super::{
    DecisionId, DecisionImplementationDescriptor, DecisionOutputDescriptor,
    DecisionParameterDescriptor,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionDescriptor {
    pub id: DecisionId,
    pub label: &'static str,
    pub parameters: &'static [DecisionParameterDescriptor],
    pub output: Option<DecisionOutputDescriptor>,
    pub error: Option<DecisionOutputDescriptor>,
    pub implementation: DecisionImplementationDescriptor,
}
