use super::{
    DecisionId, DecisionImplementationDescriptor, DecisionInputDescriptor, DecisionOutputDescriptor,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionDescriptor {
    pub id: DecisionId,
    pub label: &'static str,
    pub input: DecisionInputDescriptor,
    pub output: DecisionOutputDescriptor,
    pub implementation: DecisionImplementationDescriptor,
}
