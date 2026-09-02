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
    let domain_error_owner = assemble_domain_error_owner(domain_path, name);
    let command_owner = assemble_command_owner(domain_path, name);
    quote! {
        #aggregate_type
        #action_owner
        #public_action_owner
        #aggregate_action_owner
        #decision_owner
        #aggregate_decision_owner
        #domain_error_owner
        #command_owner
    }
}

fn assemble_command_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::CommandOwnerType for #name {
            const COMMAND_OWNER_ID: #domain_path::CommandOwnerId =
                #domain_path::CommandOwnerId::Aggregate(
                    <Self as #domain_path::AggregateType>::DESCRIPTOR.id,
                );
            const COMMAND_NAMESPACE: &'static str =
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
