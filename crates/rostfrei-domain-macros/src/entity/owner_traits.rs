use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

pub fn assemble(domain_path: &Path, name: &Ident) -> TokenStream {
    let action_owner = assemble_action_owner(domain_path, name);
    let internal_action_owner = assemble_internal_action_owner(domain_path, name);
    let entity_action_owner = assemble_entity_action_owner(domain_path, name);
    let decision_owner = assemble_decision_owner(domain_path, name);
    let entity_decision_owner = assemble_entity_decision_owner(domain_path, name);
    quote! {
        #action_owner
        #internal_action_owner
        #entity_action_owner
        #decision_owner
        #entity_decision_owner
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
