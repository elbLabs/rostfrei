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
        owner: attributes.owner,
        initial: attributes.initial,
        states,
    };
    super::validation::validate(&lifecycle)?;
    Ok(super::assembly::assemble(&lifecycle))
}
