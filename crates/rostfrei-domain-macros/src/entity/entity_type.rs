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
            const LIFECYCLE: Option<::rostfrei_domain::EntityLifecycleDescriptor> = Some(
                <#lifecycle as ::rostfrei_domain::EntityLifecycleType>::DESCRIPTOR,
            );
        }
    });
    let identity_name = &fields[identity].name;
    let identity_type = &fields[identity].base;
    let fields = crate::field::assemble_descriptors(fields);
    quote! {
        impl ::rostfrei_domain::EntityType for #name {
            type Owner = #owner;
            type Identity = #identity_type;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::rostfrei_domain::EntityDescriptor =
                ::rostfrei_domain::EntityDescriptor {
                    id: ::rostfrei_domain::EntityId {
                        aggregate: <#owner as ::rostfrei_domain::AggregateType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                    identity: ::rostfrei_domain::IdentityDescriptor {
                        field: #identity_name,
                        identity: <#identity_type as ::rostfrei_domain::DomainIdentityType>::DESCRIPTOR.id,
                    },
                    fields: #fields,
                };
            #lifecycle
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
