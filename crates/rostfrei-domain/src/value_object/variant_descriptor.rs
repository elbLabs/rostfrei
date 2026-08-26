use super::ValueObjectVariantShapeDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueObjectVariantDescriptor {
    pub name: &'static str,
    pub shape: ValueObjectVariantShapeDescriptor,
}
