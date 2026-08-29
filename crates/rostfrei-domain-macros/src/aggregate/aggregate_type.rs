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
    let decision_groups = &attributes.decisions;
    let decision_attachments = decision_groups.iter().map(|group| {
        quote! {
            impl #domain_path::AttachedDecisionGroup<#group> for #name {}
        }
    });
    let decision_assertions = (!decision_groups.is_empty()).then(|| {
        quote! {
            const _: () = {
                fn assert_owner<Group: #domain_path::DecisionGroupType<Owner = #name>>() {}
                #(let _ = assert_owner::<#decision_groups>;)*
            };
        }
    });
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
            const DECISION_GROUPS: &'static [&'static [#domain_path::DecisionDescriptor]] = &[
                #(<#decision_groups as #domain_path::DecisionGroupType>::DECISIONS,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [#domain_path::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
            const DOMAIN_EVENTS: &'static [#domain_path::DomainEventDescriptor] = &[
                #(<#events as #domain_path::DomainEventType>::DESCRIPTOR,)*
            ];
        }

        #decision_assertions
        #(#decision_attachments)*
    }
}
