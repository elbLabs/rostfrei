use super::DecisionOutcomeValueDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionOutcomeDescriptor {
    pub local_id: &'static str,
    pub label: &'static str,
    pub shape: DecisionOutcomeShapeDescriptor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutcomeShapeDescriptor {
    Unit,
    Tuple {
        fields: &'static [DecisionOutcomeValueDescriptor],
    },
    Struct {
        fields: &'static [DecisionOutcomeNamedFieldDescriptor],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecisionOutcomeNamedFieldDescriptor {
    pub name: &'static str,
    pub value: DecisionOutcomeValueDescriptor,
}
