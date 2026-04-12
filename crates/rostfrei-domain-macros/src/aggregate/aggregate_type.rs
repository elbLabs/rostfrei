use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let context = &attributes.context;
    let root = &attributes.root;
    let actions = &attributes.actions;
    let decisions = &attributes.decisions;
    let invariants = &attributes.invariants;

    quote! {
        impl ::rostfrei_domain::AggregateType for #name {
            type Context = #context;
            type Root = #root;

            const DESCRIPTOR: ::rostfrei_domain::AggregateDescriptor = {
                let id = ::rostfrei_domain::AggregateId {
                        context: <#context as ::rostfrei_domain::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                };
                ::rostfrei_domain::AggregateDescriptor {
                    id,
                    label: #label,
                    root: ::rostfrei_domain::EntityId {
                        aggregate: id,
                        local: <#root as ::rostfrei_domain::EntityType>::LOCAL_ID,
                    },
                }
            };
            const ACTION_CONTRACTS: &'static [&'static [::rostfrei_domain::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_CONTRACTS: &'static [&'static [::rostfrei_domain::DecisionDescriptor]] = &[
                #(<Self as #decisions>::__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [::rostfrei_domain::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
        }
    }
}
