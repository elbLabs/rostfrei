use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::{aggregate_type, attributes::Attributes};

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let aggregate_type = aggregate_type::assemble(domain_path, name, attributes);
    let action_owner = assemble_action_owner(domain_path, name);
    let public_action_owner = assemble_public_action_owner(domain_path, name);
    let aggregate_action_owner = assemble_aggregate_action_owner(domain_path, name);
    let decision_owner = assemble_decision_owner(domain_path, name);
    let aggregate_decision_owner = assemble_aggregate_decision_owner(domain_path, name);
    let value_object_owner = assemble_value_object_owner(domain_path, name);
    let domain_error_owner = assemble_domain_error_owner(domain_path, name);
    let domain_command_owner = assemble_domain_command_owner(domain_path, name);
    let invariant_owner = assemble_invariant_owner(domain_path, name, attributes);
    let aggregate_invariant_owner = assemble_aggregate_invariant_owner(domain_path, name);
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
            type Candidate = <Self as #domain_path::AggregateType>::Root;
            const INVARIANT_OWNER_ID: #domain_path::InvariantOwnerId =
                #domain_path::InvariantOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );

            #validate_invariants
        }
    }
}

fn assemble_aggregate_invariant_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::AggregateInvariantOwnerType for #name {}
    }
}

fn assemble_domain_command_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DomainCommandOwnerType for #name {
            const DOMAIN_COMMAND_OWNER_ID: #domain_path::DomainCommandOwnerId =
                #domain_path::DomainCommandOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
            const DOMAIN_COMMAND_NAMESPACE: &'static str =
                <Self as #domain_path::AggregateType>::DESCRIPTOR.id.context.0;
        }
    }
}

fn assemble_aggregate_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::AggregateActionOwnerType for #name {}
    }
}

fn assemble_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ActionOwnerType for #name {
            const ACTION_OWNER_ID: #domain_path::ActionOwnerId =
                #domain_path::ActionOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_aggregate_decision_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::AggregateDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: #domain_path::DecisionOwnerId =
                #domain_path::DecisionOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_public_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::PublicActionOwnerType for #name {}
    }
}

fn assemble_domain_error_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DomainErrorOwnerType for #name {
            const DOMAIN_ERROR_OWNER_ID: #domain_path::DomainErrorOwnerId =
                #domain_path::DomainErrorOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ValueObjectOwnerType for #name {
            const VALUE_OBJECT_OWNER_ID: #domain_path::ValueObjectOwnerId =
                #domain_path::ValueObjectOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
        }
    }
}
