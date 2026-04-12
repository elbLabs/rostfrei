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
        impl ::rostfrei_domain::DomainCommandOwnerType for #name {
            const DOMAIN_COMMAND_OWNER_ID: ::rostfrei_domain::DomainCommandOwnerId =
                ::rostfrei_domain::DomainCommandOwnerId::DomainService(
                    <Self as ::rostfrei_domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_domain_service_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DomainServiceActionOwnerType for #name {}
    }
}

fn assemble_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::ActionOwnerType for #name {
            const ACTION_OWNER_ID: ::rostfrei_domain::ActionOwnerId =
                ::rostfrei_domain::ActionOwnerId::DomainService(
                    <Self as ::rostfrei_domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_domain_service_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DomainServiceDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: ::rostfrei_domain::DecisionOwnerId =
                ::rostfrei_domain::DecisionOwnerId::DomainService(
                    <Self as ::rostfrei_domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_public_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::PublicActionOwnerType for #name {}
    }
}

fn assemble_domain_error_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DomainErrorOwnerType for #name {
            const DOMAIN_ERROR_OWNER_ID: ::rostfrei_domain::DomainErrorOwnerId =
                ::rostfrei_domain::DomainErrorOwnerId::DomainService(
                    <Self as ::rostfrei_domain::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}
