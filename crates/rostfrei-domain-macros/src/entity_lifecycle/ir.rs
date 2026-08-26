use proc_macro2::Span;
use syn::{Ident, LitStr, Path, TypePath};

pub struct Lifecycle {
    pub name: Ident,
    pub id: LitStr,
    pub label: LitStr,
    pub owner: TypePath,
    pub initial: Ident,
    pub states: Vec<State>,
}

pub struct State {
    pub name: Ident,
    pub id: LitStr,
    pub label: LitStr,
    pub transitions: Vec<Transition>,
}

pub struct Transition {
    pub action: ActionReferencePath,
    pub target: Ident,
}

pub struct ActionReferencePath {
    pub trait_path: Path,
    pub reference: Ident,
    pub span: Span,
    pub lexical: String,
}
