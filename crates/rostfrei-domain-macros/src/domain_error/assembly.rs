use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use crate::field::Field;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes, fields: &[Field]) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let code = &attributes.code;
    let message = &attributes.message;
    let assertions = crate::field::assemble_assertions(name, None, fields);
    let fields = crate::field::assemble_descriptors(fields);

    quote! {
        impl ::domain::DomainErrorType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::domain::DomainErrorDescriptor =
                ::domain::DomainErrorDescriptor {
                    id: ::domain::DomainErrorId {
                        owner: <#owner as ::domain::DomainErrorOwnerType>::DOMAIN_ERROR_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    code: #code,
                    message: #message,
                    fields: #fields,
                };
        }

        #assertions
    }
}
