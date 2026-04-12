use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

pub struct Arguments {
    pub group: Ident,
}

impl Parse for Arguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Err(input.error("domain_queries requires `group = GroupName`"));
        }
        let key: Ident = input.parse()?;
        if key != "group" {
            return Err(syn::Error::new(
                key.span(),
                "unsupported domain_queries argument; expected `group`",
            ));
        }
        input.parse::<Token![=]>()?;
        let group = input.parse()?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                return Ok(Self { group });
            }
            let key: Ident = input.parse()?;
            return Err(syn::Error::new(
                key.span(),
                if key == "group" {
                    "duplicate domain_queries argument `group`"
                } else {
                    "unsupported domain_queries argument; expected only `group`"
                },
            ));
        }
        if !input.is_empty() {
            return Err(input.error("domain_queries group must be a single Rust identifier"));
        }
        Ok(Self { group })
    }
}

pub fn parse(tokens: proc_macro2::TokenStream) -> syn::Result<Arguments> {
    syn::parse2(tokens)
}
