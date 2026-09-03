use super::ValueObjectId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueObjectDescriptor {
    pub id: ValueObjectId,
    pub label: &'static str,
}
