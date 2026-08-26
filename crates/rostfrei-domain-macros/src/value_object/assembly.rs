use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;
use super::ir::{Shape, VariantShape};
use super::value_object_type;

pub fn assemble(name: &Ident, attributes: &Attributes, shape: &Shape) -> TokenStream {
    let value_object_type = value_object_type::assemble(name, attributes, shape);
    let assertions = assemble_assertions(name, shape);
    let action_owner = assemble_action_owner(name);
    let internal_action_owner = assemble_internal_action_owner(name);
    let value_object_action_owner = assemble_value_object_action_owner(name);
    let decision_owner = assemble_decision_owner(name);
    let value_object_decision_owner = assemble_value_object_decision_owner(name);
    let domain_error_owner = assemble_domain_error_owner(name);
    let action_contracts = assemble_action_contracts(name);
    let decision_contracts = assemble_decision_contracts(name);
    let invariant_owner = assemble_invariant_owner(name, attributes);
    quote! {
        #value_object_type
        #assertions
        #action_owner
        #internal_action_owner
        #value_object_action_owner
        #decision_owner
        #value_object_decision_owner
        #domain_error_owner
        #action_contracts
        #decision_contracts
        #invariant_owner
    }
}

fn assemble_assertions(name: &Ident, shape: &Shape) -> TokenStream {
    match shape {
        Shape::Struct { fields } => crate::field::assemble_assertions(name, None, fields),
        Shape::Enum { .. } => crate::field::assemble_assertions(name, None, &[]),
        Shape::TaggedEnum { variants } => {
            let assertions = variants.iter().filter_map(|variant| match &variant.shape {
                VariantShape::Unit => None,
                VariantShape::Tuple { fields } | VariantShape::Struct { fields } => {
                    Some(crate::field::assemble_assertions(name, None, fields))
                }
            });
            quote!(#(#assertions)*)
        }
    }
}

fn assemble_invariant_owner(name: &Ident, attributes: &Attributes) -> TokenStream {
    let invariants = &attributes.invariants;
    quote! {
        impl ::rostfrei_domain::InvariantOwnerType for #name {
            type Candidate = Self;
            const INVARIANT_OWNER_ID: ::rostfrei_domain::InvariantOwnerId =
                ::rostfrei_domain::InvariantOwnerId::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );

            fn validate_invariants(
                candidate: &Self::Candidate,
            ) -> ::core::result::Result<
                (),
                ::std::vec::Vec<::rostfrei_domain::InvariantViolation>,
            > {
                let mut violations = ::std::vec::Vec::new();
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_APPEND_VIOLATIONS(
                    candidate,
                    &mut violations,
                );)*
                if violations.is_empty() {
                    ::core::result::Result::Ok(())
                } else {
                    ::core::result::Result::Err(violations)
                }
            }
        }

        impl ::rostfrei_domain::ValueObjectInvariantOwnerType for #name {}
    }
}

fn assemble_action_contracts(name: &Ident) -> TokenStream {
    quote! {
        impl<Owner> ::rostfrei_domain::ActionInputType<Owner> for #name {
            const DESCRIPTOR: ::rostfrei_domain::ActionInputDescriptor =
                ::rostfrei_domain::ActionInputDescriptor::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }

        impl<Contract> ::rostfrei_domain::ActionOutputType<Contract> for #name {
            const DESCRIPTOR: Option<::rostfrei_domain::ActionOutputDescriptor> = Some(
                ::rostfrei_domain::ActionOutputDescriptor::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                ),
            );
        }

        impl<Aggregate> ::rostfrei_domain::QueryInputType<Aggregate> for #name {
            const DESCRIPTOR: ::rostfrei_domain::QueryInputDescriptor =
                ::rostfrei_domain::QueryInputDescriptor::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }

        impl<Aggregate> ::rostfrei_domain::QueryOutputType<Aggregate> for #name {
            const DESCRIPTOR: ::rostfrei_domain::QueryOutputDescriptor =
                ::rostfrei_domain::QueryOutputDescriptor::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_decision_contracts(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DecisionInputType for #name {
            const DESCRIPTOR: ::rostfrei_domain::DecisionInputDescriptor =
                ::rostfrei_domain::DecisionInputDescriptor::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }

        impl ::rostfrei_domain::DecisionOutputType for #name {
            const DESCRIPTOR: ::rostfrei_domain::DecisionOutputDescriptor =
                ::rostfrei_domain::DecisionOutputDescriptor::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::ValueObjectActionOwnerType for #name {}
    }
}

fn assemble_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::ActionOwnerType for #name {
            const ACTION_OWNER_ID: ::rostfrei_domain::ActionOwnerId =
                ::rostfrei_domain::ActionOwnerId::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_value_object_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::ValueObjectDecisionOwnerType for #name {}
    }
}

fn assemble_decision_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DecisionOwnerType for #name {
            const DECISION_OWNER_ID: ::rostfrei_domain::DecisionOwnerId =
                ::rostfrei_domain::DecisionOwnerId::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}

fn assemble_internal_action_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::InternalActionOwnerType for #name {}
    }
}

fn assemble_domain_error_owner(name: &Ident) -> TokenStream {
    quote! {
        impl ::rostfrei_domain::DomainErrorOwnerType for #name {
            const DOMAIN_ERROR_OWNER_ID: ::rostfrei_domain::DomainErrorOwnerId =
                ::rostfrei_domain::DomainErrorOwnerId::ValueObject(
                    <Self as ::rostfrei_domain::ValueObjectType>::DESCRIPTOR.id,
                );
        }
    }
}
