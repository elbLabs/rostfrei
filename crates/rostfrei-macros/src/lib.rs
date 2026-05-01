use proc_macro::TokenStream;
use syn::{DeriveInput, Error, parse_macro_input};

mod query;
mod support;

#[proc_macro_derive(QueryDefinition, attributes(rostfrei))]
pub fn derive_query_definition(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    query::expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
