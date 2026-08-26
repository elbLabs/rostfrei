use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let Some(lifecycle) = &attributes.lifecycle else {
        return TokenStream::new();
    };
    quote! {
        const _: () = {
            fn assert_lifecycle<L>()
            where
                L: #domain_path::EntityLifecycleType<Owner = #name>,
            {
            }
            let _ = assert_lifecycle::<#lifecycle>;
        };
    }
}
