use syn::{Ident, LitStr};

pub struct Lifecycle {
    pub name: Ident,
    pub id: LitStr,
    pub label: LitStr,
    pub states: Vec<State>,
}

pub struct State {
    pub id: LitStr,
    pub label: LitStr,
}
