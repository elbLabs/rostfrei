use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

pub enum ContractKind {
    Aggregate,
    DomainService,
    Entity,
    ValueObject,
}

impl Parse for ContractKind {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "domain action contract kind is required; expected `aggregate`, `domain_service`, `entity`, or `value_object`",
            ));
        }

        let kind: Ident = input.parse()?;
        if input.peek(Token![=]) {
            return Err(syn::Error::new(
                kind.span(),
                "domain action contract kinds must be unkeyed; use `entity`",
            ));
        }
        if !input.is_empty() {
            return Err(input.error("domain action contract traits accept exactly one kind"));
        }

        match kind.to_string().as_str() {
            "aggregate" => Ok(Self::Aggregate),
            "domain_service" => Ok(Self::DomainService),
            "entity" => Ok(Self::Entity),
            "value_object" => Ok(Self::ValueObject),
            _ => Err(syn::Error::new(
                kind.span(),
                format!(
                    "unknown domain action contract kind `{kind}`; expected `aggregate`, `domain_service`, `entity`, or `value_object`"
                ),
            )),
        }
    }
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<ContractKind> {
    syn::parse2(tokens)
}
