use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

#[derive(Clone, Copy)]
pub enum OwnerKind {
    Aggregate,
    Entity,
    ValueObject,
}

impl Parse for OwnerKind {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error(
                "domain invariant contract kind is required; expected `aggregate`, `entity`, or `value_object`",
            ));
        }

        let kind: Ident = input.parse()?;
        if input.peek(Token![=]) {
            return Err(syn::Error::new(
                kind.span(),
                "domain invariant contract kinds must be unkeyed; use `entity`",
            ));
        }
        if !input.is_empty() {
            return Err(input.error("domain invariant contract traits accept exactly one kind"));
        }

        match kind.to_string().as_str() {
            "aggregate" => Ok(Self::Aggregate),
            "entity" => Ok(Self::Entity),
            "value_object" => Ok(Self::ValueObject),
            "domain_service" => Err(syn::Error::new(
                kind.span(),
                "domain service invariant contracts are not supported; expected `aggregate`, `entity`, or `value_object`",
            )),
            _ => Err(syn::Error::new(
                kind.span(),
                format!(
                    "unknown domain invariant contract kind `{kind}`; expected `aggregate`, `entity`, or `value_object`"
                ),
            )),
        }
    }
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<OwnerKind> {
    syn::parse2(tokens)
}
