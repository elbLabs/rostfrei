use super::ValueObjectId;
use super::ValueObjectShapeDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueObjectDescriptor {
    pub id: ValueObjectId,
    pub label: &'static str,
    pub shape: ValueObjectShapeDescriptor,
}
