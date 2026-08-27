use crate::{ScalarType, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionOutputDescriptor {
    Scalar(ScalarType),
    ValueObject(ValueObjectId),
}
