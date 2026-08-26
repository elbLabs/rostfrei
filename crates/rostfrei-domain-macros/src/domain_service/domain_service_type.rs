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
        impl ::domain::DomainServiceType for #name {
            type Context = #context;

            const DESCRIPTOR: ::domain::DomainServiceDescriptor =
                ::domain::DomainServiceDescriptor {
                    id: ::domain::DomainServiceId {
                        context: <#context as ::domain::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                };
            const ACTION_CONTRACTS: &'static [&'static [::domain::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_CONTRACTS: &'static [&'static [::domain::DecisionDescriptor]] = &[
                #(<Self as #decisions>::__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE,)*
            ];
        }
    }
}
