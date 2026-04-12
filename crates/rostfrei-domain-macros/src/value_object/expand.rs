use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let validated_input = input::validate(&input)?;
    let attributes = Attributes::parse(&input.attrs)?;
    let shape = input::normalize(validated_input)?;
    validation::validate(&attributes, &shape)?;
    Ok(assembly::assemble(&input.ident, &attributes, &shape))
}
