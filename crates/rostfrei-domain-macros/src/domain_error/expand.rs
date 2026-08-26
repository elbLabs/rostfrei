use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let fields = crate::field::extract(input::extract(&input)?)?;
    let attributes = Attributes::parse(&input.attrs)?;
    validation::validate(&attributes, &fields)?;
    Ok(assembly::assemble(&input.ident, &attributes, &fields))
}
