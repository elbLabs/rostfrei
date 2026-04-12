use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use crate::field::Field;

use super::attributes::Attributes;

pub fn assemble(
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
    identity: usize,
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let actions = &attributes.actions;
    let decisions = &attributes.decisions;
    let invariants = &attributes.invariants;
    let lifecycle = attributes.lifecycle.as_ref().map(|lifecycle| {
        quote! {
            const LIFECYCLE: Option<::domain::EntityLifecycleDescriptor> = Some(
                <#lifecycle as ::domain::EntityLifecycleType>::DESCRIPTOR,
            );
        }
    });
    let identity_name = &fields[identity].name;
    let identity_type = &fields[identity].base;
    let fields = crate::field::assemble_descriptors(fields);
    quote! {
        impl ::domain::EntityType for #name {
            type Owner = #owner;
            type Identity = #identity_type;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::domain::EntityDescriptor =
                ::domain::EntityDescriptor {
                    id: ::domain::EntityId {
                        aggregate: <#owner as ::domain::AggregateType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                    identity: ::domain::IdentityDescriptor {
                        field: #identity_name,
                        identity: <#identity_type as ::domain::DomainIdentityType>::DESCRIPTOR.id,
                    },
                    fields: #fields,
                };
            #lifecycle
            const ACTION_CONTRACTS: &'static [&'static [::domain::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_CONTRACTS: &'static [&'static [::domain::DecisionDescriptor]] = &[
                #(<Self as #decisions>::__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [::domain::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
        }
    }
}
