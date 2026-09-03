use proc_macro::TokenStream;
use syn::{DeriveInput, Error, parse_macro_input};

#[doc(hidden)]
#[proc_macro]
pub fn __install_test_macro_support(input: TokenStream) -> TokenStream {
    if !input.is_empty() {
        return Error::new(
            proc_macro2::Span::call_site(),
            "internal macro support installer does not accept arguments",
        )
        .into_compile_error()
        .into();
    }
    quote::quote! {
        #[doc(hidden)]
        pub mod __rostfrei_macro_support {
            pub mod __private {
                pub use ::zs_registry as registry;
            }
        }
    }
    .into()
}

mod query;
mod support;

#[proc_macro_derive(QueryDefinition, attributes(rostfrei))]
pub fn derive_query_definition(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    query::expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
