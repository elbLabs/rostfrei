use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::{attributes::Attributes, domain_service_type};

pub fn assemble(name: &Ident, attributes: &Attributes) -> TokenStream {
    let domain_service = domain_service_type::assemble(name, attributes);
    let action_owner = assemble_action_owner(name);
    let public_action_owner = assemble_public_action_owner(name);
    let domain_service_action_owner = assemble_domain_service_action_owner(name);
    let decision_owner = assemble_decision_owner(name);
    let domain_service_decision_owner = assemble_domain_service_decision_owner(name);
    let domain_error_owner = assemble_domain_error_owner(name);
    let domain_command_owner = assemble_domain_command_owner(name);
    quote! {
        #domain_service
        #action_owner
        #public_action_owner
        #domain_service_action_owner
        #decision_owner
        #domain_service_decision_owner
        #domain_error_owner
        #domain_command_owner
    }
}

fn assemble_domain_command_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DomainCommandOwnerType for #name {
            const DOMAIN_COMMAND_OWNER_ID: ::domain::DomainCommandOwnerId =
                ::domain::DomainCommandOwnerId::DomainService(
                    <Self as ::domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_domain_service_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DomainServiceActionOwnerType for #name {}
    }
}

fn assemble_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::ActionOwnerType for #name {
            const ACTION_OWNER_ID: ::domain::ActionOwnerId =
                ::domain::ActionOwnerId::DomainService(
                    <Self as ::domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_domain_service_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DomainServiceDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::domain::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: ::domain::DecisionOwnerId =
                ::domain::DecisionOwnerId::DomainService(
                    <Self as ::domain::DomainServiceType>::DESCRIPTOR.id,
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
                ::domain::DomainErrorOwnerId::DomainService(
                    <Self as ::domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}
