use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, input};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    input::validate(input)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    Ok(assembly::assemble(&domain_path, &input.ident))
}
