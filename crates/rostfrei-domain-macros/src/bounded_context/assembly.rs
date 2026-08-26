use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let bounded_context = assemble_bounded_context(domain_path, name, attributes);
    let value_object_owner = assemble_value_object_owner(domain_path, name);
    quote! {
        #bounded_context
        #value_object_owner
    }
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

fn assemble_value_object_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: #domain_path::ValueObjectOwnerId =
                #domain_path::ValueObjectOwnerId::BoundedContext(
                    <Self as #domain_path::BoundedContextType>::DESCRIPTOR.id,
                );
        }
    }
}
