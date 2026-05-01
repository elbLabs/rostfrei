use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    quote! {
        impl #domain_path::ValueObject for #name {
            const DESCRIPTOR: #domain_path::ValueObjectDescriptor =
                #domain_path::ValueObjectDescriptor {
                    id: #domain_path::ValueObjectId(#id),
                    label: #label,
                };
        }

    }
}
