use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let context = &attributes.context;
    let actions = &attributes.actions;
    let decisions = &attributes.decisions;

    quote! {
        impl ::rostfrei_domain::DomainServiceType for #name {
            type Context = #context;

            const DESCRIPTOR: ::rostfrei_domain::DomainServiceDescriptor =
                ::rostfrei_domain::DomainServiceDescriptor {
                    id: ::rostfrei_domain::DomainServiceId {
                        context: <#context as ::rostfrei_domain::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                };
            const ACTION_CONTRACTS: &'static [&'static [::rostfrei_domain::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_CONTRACTS: &'static [&'static [::rostfrei_domain::DecisionDescriptor]] = &[
                #(<Self as #decisions>::__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE,)*
            ];
        }
    }
}
