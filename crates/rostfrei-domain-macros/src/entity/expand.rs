use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let source_fields = input::extract(&input)?;
    let attributes = Attributes::parse(&input.attrs)?;
    let fields = crate::field::extract(source_fields)?;
    let identity = validation::validate(&attributes, &fields)?;
    Ok(assembly::assemble(
        &input.ident,
        &attributes,
        &fields,
        identity,
    ))
}
