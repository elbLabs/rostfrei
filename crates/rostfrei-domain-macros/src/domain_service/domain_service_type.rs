use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let context = &attributes.context;
    let actions = &attributes.actions;

    quote! {
        impl #domain_path::DomainServiceType for #name {
            type Context = #context;

            const DESCRIPTOR: #domain_path::DomainServiceDescriptor =
                #domain_path::DomainServiceDescriptor {
                    id: #domain_path::DomainServiceId {
                        context: <#context as #domain_path::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                };
            const ACTION_CONTRACTS: &'static [&'static [#domain_path::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
        }
    }
}
