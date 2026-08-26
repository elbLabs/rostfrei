use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let bounded_context = assemble_bounded_context(name, attributes);
    let value_object_owner = assemble_value_object_owner(name);
    quote! {
        #bounded_context
        #value_object_owner
    }
}

fn assemble_bounded_context(name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;

    quote! {
        impl ::rostfrei_domain::BoundedContextType for #name {
            const DESCRIPTOR: ::rostfrei_domain::BoundedContextDescriptor =
                ::rostfrei_domain::BoundedContextDescriptor {
                    id: ::rostfrei_domain::BoundedContextId(#id),
                    label: #label,
                };
        }
    }
}

fn assemble_value_object_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: ::rostfrei_domain::ValueObjectOwnerId =
                ::rostfrei_domain::ValueObjectOwnerId::BoundedContext(
                    <Self as ::rostfrei_domain::BoundedContextType>::DESCRIPTOR.id,
                );
        }
    }
}
