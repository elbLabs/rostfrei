use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token, parenthesized};

#[derive(Clone, Copy)]
pub enum ContractKind {
    Aggregate,
    DomainService,
    Entity,
}

pub struct ContractArguments {
    pub kind: ContractKind,
    pub instance_trait: Option<Ident>,
}

impl Parse for ContractArguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "domain action contract kind is required; expected `aggregate`, `domain_service`, or `entity`",
            ));
        }

        let kind: Ident = input.parse()?;
        if input.peek(Token![=]) {
            return Err(syn::Error::new(
                kind.span(),
                "domain action contract kinds must be unkeyed; use `entity`",
            ));
        }
        let kind = match kind.to_string().as_str() {
            "aggregate" => ContractKind::Aggregate,
            "domain_service" => ContractKind::DomainService,
            "entity" => ContractKind::Entity,
            _ => Err(syn::Error::new(
                kind.span(),
                format!(
                    "unknown domain action contract kind `{kind}`; expected `aggregate`, `domain_service`, or `entity`"
                ),
            ))?,
        };
        let instance_trait = if input.peek(syn::token::Paren) {
            let options;
            parenthesized!(options in input);
            let option: Ident = options.parse()?;
            if option != "instance" {
                return Err(syn::Error::new(
                    option.span(),
                    "unknown domain action contract option; expected `instance`",
                ));
            }
            options.parse::<Token![=]>()?;
            let instance_trait = options.parse::<Ident>()?;
            if options.peek(Token![,]) {
                options.parse::<Token![,]>()?;
            }
            if !options.is_empty() {
                return Err(
                    options.error("domain action contract accepts only one `instance` option")
                );
            }
            Some(instance_trait)
        } else {
            None
        };
        if !input.is_empty() {
            return Err(input.error("domain action contract traits accept exactly one kind"));
        }
        if let Some(instance_trait) = &instance_trait
            && !matches!(kind, ContractKind::Aggregate)
        {
            return Err(syn::Error::new(
                instance_trait.span(),
                "the `instance` option is supported only for aggregate action contracts",
            ));
        }

        Ok(Self {
            kind,
            instance_trait,
        })
    }
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<ContractArguments> {
    syn::parse2(tokens)
}
