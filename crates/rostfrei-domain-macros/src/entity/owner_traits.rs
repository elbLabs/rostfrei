use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let action_owner = assemble_action_owner(name);
    let internal_action_owner = assemble_internal_action_owner(name);
    let entity_action_owner = assemble_entity_action_owner(name);
    let decision_owner = assemble_decision_owner(name);
    let entity_decision_owner = assemble_entity_decision_owner(name);
    let value_object_owner = assemble_value_object_owner(name);
    let domain_error_owner = assemble_domain_error_owner(name);
    let invariant_owner = assemble_invariant_owner(name, attributes);
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

fn assemble_invariant_owner(name: &Ident, attributes: &Attributes) -> TokenStream {
    let invariants = &attributes.invariants;
    let validate_invariants = if invariants.is_empty() {
        quote! {
            fn validate_invariants(
                _candidate: &Self::Candidate,
            ) -> ::core::result::Result<
                (),
                ::std::vec::Vec<::domain::InvariantViolation>,
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
                ::std::vec::Vec<::domain::InvariantViolation>,
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
        impl ::domain::InvariantOwnerType for #name {
            type Candidate = Self;
            const INVARIANT_OWNER_ID: ::domain::InvariantOwnerId =
                ::domain::InvariantOwnerId::Entity(
                    <Self as ::domain::EntityType>::DESCRIPTOR.id,
                );

            #validate_invariants
        }

        impl ::domain::EntityInvariantOwnerType for #name {}
    }
}

fn assemble_entity_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::EntityActionOwnerType for #name {}
    }
}

fn assemble_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::ActionOwnerType for #name {
            const ACTION_OWNER_ID: ::domain::ActionOwnerId =
                ::domain::ActionOwnerId::Entity(
                    <Self as ::domain::EntityType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_entity_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::EntityDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: ::domain::DecisionOwnerId =
                ::domain::DecisionOwnerId::Entity(
                    <Self as ::domain::EntityType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_internal_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::InternalActionOwnerType for #name {}
    }
}

fn assemble_domain_error_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DomainErrorOwnerType for #name {
            const DOMAIN_ERROR_OWNER_ID: ::domain::DomainErrorOwnerId =
                ::domain::DomainErrorOwnerId::Entity(
                    <Self as ::domain::EntityType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: ::domain::ValueObjectOwnerId =
                ::domain::ValueObjectOwnerId::Entity(
                    ::domain::EntityId {
                        aggregate: <<Self as ::domain::EntityType>::Owner as ::domain::AggregateType>::DESCRIPTOR.id,
                        local: <Self as ::domain::EntityType>::LOCAL_ID,
                    },
                );
        }
    }
}
