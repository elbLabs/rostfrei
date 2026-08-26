use proc_macro2::TokenStream;
use syn::DeriveInput;

use super::{assembly, attributes::Attributes, input, validation};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    input::validate(&input)?;
    let attributes = Attributes::parse(&input.attrs)?;
    validation::validate(&attributes)?;
    Ok(assembly::assemble(&input.ident, &attributes))
}
