use syn::{Attribute, Ident, LitStr, Type, Visibility};

pub struct Parameter {
    pub name: Ident,
    pub signature_type: Type,
    pub descriptor_type: Type,
}

pub struct Decision {
    pub name: Ident,
    pub visibility: Visibility,
    pub cfg_attributes: Vec<Attribute>,
    pub id: LitStr,
    pub label: LitStr,
    pub parameters: Vec<Parameter>,
    pub return_type: Type,
}
