use super::FieldValue;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FieldDescriptor {
    pub name: &'static str,
    pub value: FieldValue,
}
