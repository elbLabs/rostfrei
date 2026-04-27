use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let variants = super::input::extract(input)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    let descriptor = super::assembly::assemble(&domain_path, &input.ident, &variants);
    let runtime = crate::helper::runtime_api_path::resolve_optional()?.map_or_else(
        TokenStream::new,
        |runtime_path| {
            super::runtime::assemble(&domain_path, &runtime_path, &input.ident, &variants)
        },
    );
    Ok(quote! {
        #descriptor
        #runtime
    })
}
