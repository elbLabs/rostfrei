use crate::{ScalarType, ValueObjectId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionInputDescriptor {
    Scalar(ScalarType),
    ValueObject(ValueObjectId),
}
