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
        impl ::domain::BoundedContextType for #name {
            const DESCRIPTOR: ::domain::BoundedContextDescriptor =
                ::domain::BoundedContextDescriptor {
                    id: ::domain::BoundedContextId(#id),
                    label: #label,
                };
        }
    }
}

fn assemble_value_object_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: ::domain::ValueObjectOwnerId =
                ::domain::ValueObjectOwnerId::BoundedContext(
                    <Self as ::domain::BoundedContextType>::DESCRIPTOR.id,
                );
        }
    }
}
