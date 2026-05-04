use syn::{Ident, LitStr, TypePath};

pub struct TransitionSet {
    pub name: Ident,
    pub state: TypePath,
    pub transitions: Vec<Transition>,
}

pub struct Transition {
    pub name: Ident,
    pub id: LitStr,
    pub label: LitStr,
    pub from: Ident,
    pub to: Ident,
}
