use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let Some(lifecycle) = &attributes.lifecycle else {
        return TokenStream::new();
    };
    quote! {
        const _: () = {
            fn assert_lifecycle<L>()
            where
                L: ::rostfrei_domain::EntityLifecycleType<Owner = #name>,
            {
            }
            let _ = assert_lifecycle::<#lifecycle>;
        };
    }
}
