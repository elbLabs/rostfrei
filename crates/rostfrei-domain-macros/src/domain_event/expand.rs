use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let fields = crate::field::extract(input::extract(input)?)?;
    let attributes = Attributes::parse(&input.attrs)?;
    validation::validate(&attributes, &fields)?;
    let domain_path = crate::helper::domain_api_path::resolve()?;
    Ok(assembly::assemble(
        &domain_path,
        &input.ident,
        &attributes,
        &fields,
    ))
}
