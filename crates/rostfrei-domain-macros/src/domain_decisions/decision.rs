use syn::{Attribute, Ident, LitStr, Type, Visibility};

pub struct Decision {
    pub name: Ident,
    pub visibility: Visibility,
    pub cfg_attributes: Vec<Attribute>,
    pub id: LitStr,
    pub label: LitStr,
    pub parameters: Vec<Type>,
    pub return_type: Type,
}
