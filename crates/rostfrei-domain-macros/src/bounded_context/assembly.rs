use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    assemble_bounded_context(domain_path, name, attributes)
}

fn assemble_bounded_context(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;

    quote! {
        impl #domain_path::BoundedContextType for #name {
            const DESCRIPTOR: #domain_path::BoundedContextDescriptor =
                #domain_path::BoundedContextDescriptor {
                    id: #domain_path::BoundedContextId(#id),
                    label: #label,
                };
        }
    }
}
