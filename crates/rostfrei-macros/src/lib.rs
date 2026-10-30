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

mod event_codec;
mod query;
mod read_model;
mod support;

/// Composes named historical event conversions through the runtime codec builder.
///
/// Apply `#[event_codec(aggregate = Inventory)]` to an inherent impl and mark
/// conversion methods with `#[upcast]`. Install `rostfrei::install_macro_support!()`
/// at the consuming crate root, then explicitly register `Codec::builder().build()?`
/// in `AggregateCodecs`. Fallible methods use `Result<CurrentEvent, Error>`.
///
/// See the [schema-evolution guide](https://elblabs.github.io/rostfrei/docs/event-schema-evolution)
/// for a complete declaration, manual builder, and compatibility-release encoder.
#[proc_macro_attribute]
pub fn event_codec(args: TokenStream, input: TokenStream) -> TokenStream {
    event_codec::expand(args.into(), input.into())
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(QueryDefinition, attributes(rostfrei))]
pub fn derive_query_definition(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    query::expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

#[proc_macro_derive(ReadModel, attributes(read_model))]
pub fn derive_read_model(input: TokenStream) -> TokenStream {
    read_model::expand(&parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(Error::into_compile_error)
        .into()
}
