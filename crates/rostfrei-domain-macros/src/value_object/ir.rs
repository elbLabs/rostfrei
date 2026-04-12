use crate::field::Field;

pub enum Shape {
    Struct { fields: Vec<Field> },
    Enum { variants: Vec<String> },
    TaggedEnum { variants: Vec<Variant> },
}

pub struct Variant {
    pub name: String,
    pub shape: VariantShape,
}

pub enum VariantShape {
    Unit,
    Tuple { fields: Vec<Field> },
    Struct { fields: Vec<Field> },
}
