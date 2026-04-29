use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::{attributes::Attributes, domain_service_type};

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let domain_service = domain_service_type::assemble(domain_path, name, attributes);
    let action_owner = assemble_action_owner(domain_path, name);
    let public_action_owner = assemble_public_action_owner(domain_path, name);
    let domain_service_action_owner = assemble_domain_service_action_owner(domain_path, name);
    quote! {
        #domain_service
        #action_owner
        #public_action_owner
        #domain_service_action_owner
    }
}

fn assemble_domain_service_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DomainServiceActionOwnerType for #name {}
    }
}

fn assemble_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ActionOwnerType for #name {
            const ACTION_OWNER_ID: #domain_path::ActionOwnerId =
                #domain_path::ActionOwnerId::DomainService(
                    <Self as #domain_path::DomainServiceType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_public_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::PublicActionOwnerType for #name {}
    }
}
