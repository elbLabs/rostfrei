use syn::{Attribute, LitStr, Type};

pub struct Outcome {
    pub local_id: LitStr,
    pub label: LitStr,
    pub shape: Shape,
    pub cfg_attributes: Vec<Attribute>,
}

pub enum Shape {
    Unit,
    Tuple { fields: Vec<ValueField> },
    Struct { fields: Vec<NamedField> },
}

pub struct ValueField {
    pub ty: Type,
    pub cfg_attributes: Vec<Attribute>,
}

pub struct NamedField {
    pub name: LitStr,
    pub value: ValueField,
}
