use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;
use super::ir::{Shape, VariantShape};
use super::value_object_type;

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    shape: &Shape,
) -> TokenStream {
    let value_object_type = value_object_type::assemble(domain_path, name, attributes, shape);
    let assertions = assemble_assertions(domain_path, name, shape);
    let action_owner = assemble_action_owner(domain_path, name);
    let internal_action_owner = assemble_internal_action_owner(domain_path, name);
    let value_object_action_owner = assemble_value_object_action_owner(domain_path, name);
    let domain_error_owner = assemble_domain_error_owner(domain_path, name);
    let action_contracts = assemble_action_contracts(domain_path, name);
    let decision_contracts = assemble_decision_contracts(domain_path, name);
    quote! {
        #value_object_type
        #assertions
        #action_owner
        #internal_action_owner
        #value_object_action_owner
        #domain_error_owner
        #action_contracts
        #decision_contracts
    }
}

fn assemble_assertions(domain_path: &Path, name: &Ident, shape: &Shape) -> TokenStream {
    match shape {
        Shape::Struct { fields } => {
            crate::field::assemble_assertions_with_path(domain_path, name, None, fields)
        }
        Shape::Enum { .. } => {
            crate::field::assemble_assertions_with_path(domain_path, name, None, &[])
        }
        Shape::TaggedEnum { variants } => {
            let assertions = variants.iter().filter_map(|variant| match &variant.shape {
                VariantShape::Unit => None,
                VariantShape::Tuple { fields } | VariantShape::Struct { fields } => Some(
                    crate::field::assemble_assertions_with_path(domain_path, name, None, fields),
                ),
            });
            quote!(#(#assertions)*)
        }
    }
}

fn assemble_action_contracts(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl<Owner> #domain_path::ActionInputType<Owner> for #name {
            const DESCRIPTOR: #domain_path::ActionInputDescriptor =
                #domain_path::ActionInputDescriptor::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                );
        }

        impl<Contract> #domain_path::ActionOutputType<Contract> for #name {
            const DESCRIPTOR: Option<#domain_path::ActionOutputDescriptor> = Some(
                #domain_path::ActionOutputDescriptor::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                ),
            );
        }

        impl<Aggregate> #domain_path::QueryInputType<Aggregate> for #name {
            const DESCRIPTOR: #domain_path::QueryInputDescriptor =
                #domain_path::QueryInputDescriptor::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                );
        }

        impl<Aggregate> #domain_path::QueryOutputType<Aggregate> for #name {
            const DESCRIPTOR: #domain_path::QueryOutputDescriptor =
                #domain_path::QueryOutputDescriptor::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_decision_contracts(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::DecisionInputType for #name {
            const DESCRIPTOR: #domain_path::DecisionInputDescriptor =
                #domain_path::DecisionInputDescriptor::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                );
        }

        impl #domain_path::DecisionOutcomeValueType for #name {
            const DESCRIPTOR: #domain_path::DecisionOutcomeValueDescriptor =
                #domain_path::DecisionOutcomeValueDescriptor::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ValueObjectActionOwnerType for #name {}
    }
}

fn assemble_action_owner(domain_path: &Path, name: &Ident) -> TokenStream {
    quote! {
        impl #domain_path::ActionOwnerType for #name {
            const ACTION_OWNER_ID: #domain_path::ActionOwnerId =
                #domain_path::ActionOwnerId::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
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
                #domain_path::DomainErrorOwnerId::ValueObject(
                    <Self as #domain_path::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}
