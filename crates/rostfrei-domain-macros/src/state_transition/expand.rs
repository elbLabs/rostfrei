use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::ir::TransitionSet;

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let data = super::input::extract(&input)?;
    let attributes = super::attributes::parse(&input.attrs)?;
    let transitions = super::collection::collect(data)?;
    let transition_set = TransitionSet {
        name: input.ident,
        state: attributes.state,
        transitions,
    };
    super::validation::validate(&transition_set)?;
    let domain_path = crate::helper::domain_api_path::resolve();
    Ok(super::assembly::assemble(&domain_path, &transition_set))
}
