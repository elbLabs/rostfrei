use syn::{LitStr, Type};

pub struct Decision {
    pub id: LitStr,
    pub label: LitStr,
    pub input: Type,
    pub output: Type,
}
