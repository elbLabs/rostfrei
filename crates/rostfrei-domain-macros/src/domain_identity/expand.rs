use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes, input};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let value = input::extract(&input)?;
    let attributes = attributes::parse(&input.attrs)?;
    input::validate_value(&value, attributes.scalar.is_some())?;
    Ok(assembly::assemble(
        &input.ident,
        &attributes.owner,
        &value,
        attributes.scalar.as_ref(),
    ))
}
