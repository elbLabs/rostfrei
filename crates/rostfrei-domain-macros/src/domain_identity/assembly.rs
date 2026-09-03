use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

pub fn assemble(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DomainIdentity for #name {}
    }
}
