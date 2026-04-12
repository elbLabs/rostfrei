use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use crate::field::Field;

use super::attributes::Attributes;

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let code = &attributes.code;
    let message = &attributes.message;
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);

    quote! {
        impl #domain_path::DomainErrorType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::DomainErrorDescriptor =
                #domain_path::DomainErrorDescriptor {
                    id: #domain_path::DomainErrorId {
                        owner: <#owner as #domain_path::DomainErrorOwnerType>::DOMAIN_ERROR_OWNER_ID,
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
