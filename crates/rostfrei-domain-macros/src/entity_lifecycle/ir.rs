use syn::{Ident, LitStr};

pub struct Lifecycle {
    pub name: Ident,
    pub id: LitStr,
    pub label: LitStr,
    pub initial: Ident,
    pub states: Vec<State>,
}

pub struct State {
    pub name: Ident,
    pub id: LitStr,
    pub label: LitStr,
}
