use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

#[derive(Clone, Copy)]
pub enum OwnerKind {
    Aggregate,
    Entity,
}

impl Parse for OwnerKind {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error(
                "domain decision owner kind is required; expected `aggregate` or `entity`",
            ));
        }
        let kind: Ident = input.parse()?;
        if input.peek(Token![=]) {
            return Err(syn::Error::new(
                kind.span(),
                "domain decision owner kinds must be unkeyed; use `entity`",
            ));
        }
        if !input.is_empty() {
            return Err(input.error("domain decision impl blocks accept exactly one owner kind"));
        }
        match kind.to_string().as_str() {
            "aggregate" => Ok(Self::Aggregate),
            "entity" => Ok(Self::Entity),
            _ => Err(syn::Error::new(
                kind.span(),
                format!(
                    "unknown domain decision owner kind `{kind}`; expected `aggregate` or `entity`"
                ),
            )),
        }
    }
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<OwnerKind> {
    syn::parse2(tokens)
}
