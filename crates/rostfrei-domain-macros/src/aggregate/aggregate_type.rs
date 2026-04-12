use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let context = &attributes.context;
    let root = &attributes.root;
    let actions = &attributes.actions;
    let decisions = &attributes.decisions;
    let invariants = &attributes.invariants;
    let events = attributes.events.iter().flatten();

    quote! {
        impl #domain_path::AggregateType for #name {
            type Context = #context;
            type Root = #root;

            const DESCRIPTOR: #domain_path::AggregateDescriptor = {
                let id = #domain_path::AggregateId {
                        context: <#context as #domain_path::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                };
                #domain_path::AggregateDescriptor {
                    id,
                    label: #label,
                    root: #domain_path::EntityId {
                        aggregate: id,
                        local: <#root as #domain_path::EntityType>::LOCAL_ID,
                    },
                }
            };
            const ACTION_CONTRACTS: &'static [&'static [#domain_path::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_CONTRACTS: &'static [&'static [#domain_path::DecisionDescriptor]] = &[
                #(<Self as #decisions>::__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [#domain_path::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
            const DOMAIN_EVENTS: &'static [#domain_path::DomainEventDescriptor] = &[
                #(<#events as #domain_path::DomainEventType>::DESCRIPTOR,)*
            ];
        }
    }
}
