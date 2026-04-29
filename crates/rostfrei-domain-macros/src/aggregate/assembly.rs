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
    quote! {
        #aggregate_type
        #action_owner
        #public_action_owner
        #aggregate_action_owner
        #decision_owner
        #aggregate_decision_owner
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
