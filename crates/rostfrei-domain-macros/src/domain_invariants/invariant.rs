use syn::{Ident, LitStr};

pub struct Invariant {
    pub id: LitStr,
    pub label: LitStr,
    pub method: Ident,
}
