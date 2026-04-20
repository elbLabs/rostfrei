use super::DecisionInputDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionParameterDescriptor {
    pub name: &'static str,
    pub input: DecisionInputDescriptor,
}
