use crate::ValueObjectId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutputDescriptor {
    ValueObject(ValueObjectId),
}
