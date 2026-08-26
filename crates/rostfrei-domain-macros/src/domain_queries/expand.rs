use proc_macro2::TokenStream;

use super::{arguments, assembly, attributes, input, validation};

pub fn expand(args: TokenStream, tokens: TokenStream) -> syn::Result<TokenStream> {
    let arguments = arguments::parse(args)?;
    let mut input = input::parse(tokens)?;
    let mut queries = attributes::extract(&mut input.item.items)?;
    validation::validate(&mut queries)?;
    Ok(assembly::assemble(
        input.item,
        &input.owner,
        &arguments.group,
        &queries,
    ))
}
