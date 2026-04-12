use crate::FieldDescriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueObjectVariantShapeDescriptor {
    Unit,
    Tuple { fields: &'static [FieldDescriptor] },
    Struct { fields: &'static [FieldDescriptor] },
}
