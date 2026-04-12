use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

#[derive(Clone, Copy)]
pub enum OwnerKind {
    Aggregate,
    DomainService,
    Entity,
    ValueObject,
}

impl Parse for OwnerKind {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error(
                "domain decision contract kind is required; expected `aggregate`, `domain_service`, `entity`, or `value_object`",
            ));
        }

        let kind: Ident = input.parse()?;
        if input.peek(Token![=]) {
            return Err(syn::Error::new(
                kind.span(),
                "domain decision contract kinds must be unkeyed; use `entity`",
            ));
        }
        if !input.is_empty() {
            return Err(input.error("domain decision contract traits accept exactly one kind"));
        }

        match kind.to_string().as_str() {
            "aggregate" => Ok(Self::Aggregate),
            "domain_service" => Ok(Self::DomainService),
            "entity" => Ok(Self::Entity),
            "value_object" => Ok(Self::ValueObject),
            _ => Err(syn::Error::new(
                kind.span(),
                format!(
                    "unknown domain decision contract kind `{kind}`; expected `aggregate`, `domain_service`, `entity`, or `value_object`"
                ),
            )),
        }
    }
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<OwnerKind> {
    syn::parse2(tokens)
}
