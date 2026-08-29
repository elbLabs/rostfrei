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
    identity: &Field,
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
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
    let lifecycle = attributes.lifecycle.as_ref().map(|lifecycle| {
        quote! {
            const LIFECYCLE: Option<#domain_path::EntityLifecycleDescriptor> = Some(
                <#lifecycle as #domain_path::EntityLifecycleType>::DESCRIPTOR,
            );
        }
    });
    let identity_name = &identity.name;
    let identity_type = &identity.base;
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);
    quote! {
        impl #domain_path::EntityType for #name {
            type Owner = #owner;
            type Identity = #identity_type;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::EntityDescriptor =
                #domain_path::EntityDescriptor {
                    id: #domain_path::EntityId {
                        aggregate: <#owner as #domain_path::AggregateType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                    identity: #domain_path::IdentityDescriptor {
                        field: #identity_name,
                        identity: <#identity_type as #domain_path::DomainIdentityType>::DESCRIPTOR.id,
                    },
                    fields: #fields,
                };
            #lifecycle
            const ACTION_CONTRACTS: &'static [&'static [#domain_path::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_GROUPS: &'static [&'static [#domain_path::DecisionDescriptor]] = &[
                #(<#decision_groups as #domain_path::DecisionGroupType>::DECISIONS,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [#domain_path::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
        }

        #decision_assertions
        #(#decision_attachments)*
    }
}
