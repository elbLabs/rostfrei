use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::{aggregate_type, attributes::Attributes};

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let aggregate_type = aggregate_type::assemble(name, attributes);
    let action_owner = assemble_action_owner(name);
    let public_action_owner = assemble_public_action_owner(name);
    let aggregate_action_owner = assemble_aggregate_action_owner(name);
    let decision_owner = assemble_decision_owner(name);
    let aggregate_decision_owner = assemble_aggregate_decision_owner(name);
    let value_object_owner = assemble_value_object_owner(name);
    let domain_error_owner = assemble_domain_error_owner(name);
    let domain_command_owner = assemble_domain_command_owner(name);
    let invariant_owner = assemble_invariant_owner(name, attributes);
    let aggregate_invariant_owner = assemble_aggregate_invariant_owner(name);
    quote! {
        #aggregate_type
        #action_owner
        #public_action_owner
        #aggregate_action_owner
        #decision_owner
        #aggregate_decision_owner
        #value_object_owner
        #domain_error_owner
        #domain_command_owner
        #invariant_owner
        #aggregate_invariant_owner
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
            type Candidate = <Self as ::domain::AggregateType>::Root;
            const INVARIANT_OWNER_ID: ::domain::InvariantOwnerId =
                ::domain::InvariantOwnerId::Aggregate(
                    <Self as ::domain::AggregateType>::DESCRIPTOR.id,
                );

            #validate_invariants
        }
    }
}

fn assemble_aggregate_invariant_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::AggregateInvariantOwnerType for #name {}
    }
}

fn assemble_domain_command_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DomainCommandOwnerType for #name {
            const DOMAIN_COMMAND_OWNER_ID: ::domain::DomainCommandOwnerId =
                ::domain::DomainCommandOwnerId::Aggregate(
                    <Self as ::domain::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_aggregate_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::AggregateActionOwnerType for #name {}
    }
}

fn assemble_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::ActionOwnerType for #name {
            const ACTION_OWNER_ID: ::domain::ActionOwnerId =
                ::domain::ActionOwnerId::Aggregate(
                    <Self as ::domain::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_aggregate_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::AggregateDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: ::domain::DecisionOwnerId =
                ::domain::DecisionOwnerId::Aggregate(
                    <Self as ::domain::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_public_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::PublicActionOwnerType for #name {}
    }
}

fn assemble_domain_error_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DomainErrorOwnerType for #name {
            const DOMAIN_ERROR_OWNER_ID: ::domain::DomainErrorOwnerId =
                ::domain::DomainErrorOwnerId::Aggregate(
                    <Self as ::domain::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: ::domain::ValueObjectOwnerId =
                ::domain::ValueObjectOwnerId::Aggregate(
                    <Self as ::domain::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}
