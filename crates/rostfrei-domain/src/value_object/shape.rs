use crate::{FieldDescriptor, ValueObjectVariantDescriptor};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueObjectShapeDescriptor {
    Struct {
        fields: &'static [FieldDescriptor],
    },
    Enum {
        variants: &'static [&'static str],
    },
    TaggedEnum {
        variants: &'static [ValueObjectVariantDescriptor],
    },
}
