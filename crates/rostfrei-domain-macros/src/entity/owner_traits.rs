use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let action_owner = assemble_action_owner(domain_path, name);
    let internal_action_owner = assemble_internal_action_owner(domain_path, name);
    let entity_action_owner = assemble_entity_action_owner(domain_path, name);
    let decision_owner = assemble_decision_owner(domain_path, name);
    let entity_decision_owner = assemble_entity_decision_owner(domain_path, name);
    let value_object_owner = assemble_value_object_owner(domain_path, name);
    let domain_error_owner = assemble_domain_error_owner(domain_path, name);
    let invariant_owner = assemble_invariant_owner(domain_path, name, attributes);
    quote! {
        #action_owner
        #internal_action_owner
        #entity_action_owner
        #decision_owner
        #entity_decision_owner
        #value_object_owner
        #domain_error_owner
        #invariant_owner
    }
}

fn assemble_invariant_owner(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
) -> TokenStream {
    let invariants = &attributes.invariants;
    let validate_invariants = if invariants.is_empty() {
        quote! {
            fn validate_invariants(
                _candidate: &Self::Candidate,
            ) -> ::core::result::Result<
                (),
                ::std::vec::Vec<#domain_path::InvariantViolation>,
            > {
                ::core::result::Result::Ok(())
            }
        }
    } else {
        quote! {
            fn validate_invariants(
                candidate: &Self::Candidate,
            ) -> ::core::result::Result<
                (),
                ::std::vec::Vec<#domain_path::InvariantViolation>,
            > {
                let mut violations = ::std::vec::Vec::new();
                #(
                    <Self as #invariants>::__DOMAIN_INVARIANTS_APPEND_VIOLATIONS(
                        candidate,
                        &mut violations,
                    );
                )*
                if violations.is_empty() {
                    ::core::result::Result::Ok(())
                } else {
                    ::core::result::Result::Err(violations)
                }
            }
        }
    };

    quote! {
        impl #domain_path::InvariantOwnerType for #name {
            type Candidate = Self;
            const INVARIANT_OWNER_ID: #domain_path::InvariantOwnerId =
                #domain_path::InvariantOwnerId::Entity(
                    <Self as #domain_path::EntityType>::DESCRIPTOR.id,
                );

            #validate_invariants
        }

        impl #domain_path::EntityInvariantOwnerType for #name {}
    }
}

fn assemble_entity_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::EntityActionOwnerType for #name {}
    }
}

fn assemble_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ActionOwnerType for #name {
            const ACTION_OWNER_ID: #domain_path::ActionOwnerId =
                #domain_path::ActionOwnerId::Entity(
                    <Self as #domain_path::EntityType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_entity_decision_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::EntityDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: #domain_path::DecisionOwnerId =
                #domain_path::DecisionOwnerId::Entity(
                    <Self as #domain_path::EntityType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_internal_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::InternalActionOwnerType for #name {}
    }
}

fn assemble_domain_error_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DomainErrorOwnerType for #name {
            const DOMAIN_ERROR_OWNER_ID: #domain_path::DomainErrorOwnerId =
                #domain_path::DomainErrorOwnerId::Entity(
                    <Self as #domain_path::EntityType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: #domain_path::ValueObjectOwnerId =
                #domain_path::ValueObjectOwnerId::Entity(
                    #domain_path::EntityId {
                        aggregate: <<Self as #domain_path::EntityType>::Owner as #domain_path::AggregateType>::DESCRIPTOR.id,
                        local: <Self as #domain_path::EntityType>::LOCAL_ID,
                    },
                );
        }
    }
}
