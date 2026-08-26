use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;
use crate::field::Field;

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let descriptors = crate::field::assemble_descriptors_with_path(domain_path, fields);
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    quote! {
        impl #domain_path::DomainCommandType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::DomainCommandDescriptor =
                #domain_path::DomainCommandDescriptor {
                    id: #domain_path::DomainCommandId {
                        owner: <#owner as #domain_path::DomainCommandOwnerType>::DOMAIN_COMMAND_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    fields: #descriptors,
                };
        }

        impl #domain_path::ActionInputType<#owner> for #name {
            const DESCRIPTOR: #domain_path::ActionInputDescriptor =
                #domain_path::ActionInputDescriptor::DomainCommand(
                    <Self as #domain_path::DomainCommandType>::DESCRIPTOR.id,
                );
        }

        #assertions
    }
}
