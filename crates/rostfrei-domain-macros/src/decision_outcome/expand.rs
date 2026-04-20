use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, input, validation};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let data = input::validate(input)?;
    let outcomes = input::normalize(data)?;
    validation::validate(&outcomes)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    Ok(assembly::assemble(&domain_path, &input.ident, &outcomes))
}
