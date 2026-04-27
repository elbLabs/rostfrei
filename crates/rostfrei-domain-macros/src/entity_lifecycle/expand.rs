use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::ir::Lifecycle;

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let data = super::input::extract(&input)?;
    let attributes = super::attributes::parse(&input.attrs)?;
    let states = super::collection::collect(data)?;
    let lifecycle = Lifecycle {
        name: input.ident,
        id: attributes.id,
        label: attributes.label,
        states,
    };
    super::validation::validate(&lifecycle)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    Ok(super::assembly::assemble(&domain_path, &lifecycle))
}
