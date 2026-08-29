use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    input::validate(input)?;
    let attributes = Attributes::parse(&input.attrs)?;
    validation::validate(&attributes)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    let descriptor = assembly::assemble(&domain_path, &input.ident, &attributes);
    let (events, runtime) = match attributes.events.as_deref() {
        Some(registered_events) => {
            let events = super::events::assemble(&domain_path, &input.ident, registered_events);
            let runtime = crate::helper::runtime_api_path::resolve_optional()?.map_or_else(
                TokenStream::new,
                |runtime_path| {
                    super::runtime::assemble(
                        &domain_path,
                        &runtime_path,
                        &input.ident,
                        &input.vis,
                        &attributes.root,
                        registered_events,
                    )
                },
            );
            (events, runtime)
        }
        None => (TokenStream::new(), TokenStream::new()),
    };
    Ok(quote::quote! {
        #descriptor
        #events
        #runtime
    })
}
