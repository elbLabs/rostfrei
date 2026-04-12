use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;
use crate::field::Field;

pub fn assemble(name: &Ident, attributes: &Attributes, fields: &[Field]) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let descriptors = crate::field::assemble_descriptors(fields);
    let assertions = crate::field::assemble_assertions(name, None, fields);
    quote! {
        impl ::rostfrei_domain::DomainCommandType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::rostfrei_domain::DomainCommandDescriptor =
                ::rostfrei_domain::DomainCommandDescriptor {
                    id: ::rostfrei_domain::DomainCommandId {
                        owner: <#owner as ::rostfrei_domain::DomainCommandOwnerType>::DOMAIN_COMMAND_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    fields: #descriptors,
                };
        }

        impl ::rostfrei_domain::ActionInputType<#owner> for #name {
            const DESCRIPTOR: ::rostfrei_domain::ActionInputDescriptor =
                ::rostfrei_domain::ActionInputDescriptor::DomainCommand(
                    <Self as ::rostfrei_domain::DomainCommandType>::DESCRIPTOR.id,
                );
        }

        #assertions
    }
}
