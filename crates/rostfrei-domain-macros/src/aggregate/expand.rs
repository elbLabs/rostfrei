use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    input::validate(&input)?;
    let attributes = Attributes::parse(&input.attrs)?;
    validation::validate(&attributes)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    let descriptor = assembly::assemble(&domain_path, &input.ident, &attributes);
    let events = attributes
        .events
        .as_ref()
        .map_or_else(TokenStream::new, |_| {
            super::events::assemble(&domain_path, &input.ident, &attributes)
        });
    let runtime = if attributes.events.is_some() {
        crate::helper::runtime_api_path::resolve_optional()?.map_or_else(
            TokenStream::new,
            |runtime_path| {
                super::runtime::assemble(
                    &domain_path,
                    &runtime_path,
                    &input.ident,
                    &input.vis,
                    &attributes,
                )
            },
        )
    } else {
        TokenStream::new()
    };
    Ok(quote::quote! {
        #descriptor
        #events
        #runtime
    })
}
