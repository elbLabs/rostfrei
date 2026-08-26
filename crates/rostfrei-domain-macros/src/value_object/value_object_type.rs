use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use super::attributes::Attributes;
use super::ir::{Shape, Variant, VariantShape};

pub fn assemble(name: &Ident, attributes: &Attributes, shape: &Shape) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let actions = &attributes.actions;
    let decisions = &attributes.decisions;
    let invariants = &attributes.invariants;
    let shape = assemble_shape(shape);
    quote! {
        impl ::rostfrei_domain::ValueObjectType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: ::rostfrei_domain::ValueObjectDescriptor =
                ::rostfrei_domain::ValueObjectDescriptor {
                    id: ::rostfrei_domain::ValueObjectId {
                        owner: <#owner as ::rostfrei_domain::ValueObjectOwnerType>::VALUE_OBJECT_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    shape: #shape,
                };
            const ACTION_CONTRACTS: &'static [&'static [::rostfrei_domain::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const DECISION_CONTRACTS: &'static [&'static [::rostfrei_domain::DecisionDescriptor]] = &[
                #(<Self as #decisions>::__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [::rostfrei_domain::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
        }
    }
}

fn assemble_shape(shape: &Shape) -> TokenStream {
    match shape {
        Shape::Struct { fields } => {
            let fields = crate::field::assemble_descriptors(fields);
            quote!(::rostfrei_domain::ValueObjectShapeDescriptor::Struct { fields: #fields })
        }
        Shape::Enum { variants } => {
            quote!(::rostfrei_domain::ValueObjectShapeDescriptor::Enum { variants: &[#(#variants),*] })
        }
        Shape::TaggedEnum { variants } => {
            let variants = variants.iter().map(assemble_variant);
            quote!(::rostfrei_domain::ValueObjectShapeDescriptor::TaggedEnum { variants: &[#(#variants),*] })
        }
    }
}

fn assemble_variant(variant: &Variant) -> TokenStream {
    let name = &variant.name;
    let shape = match &variant.shape {
        VariantShape::Unit => quote!(::rostfrei_domain::ValueObjectVariantShapeDescriptor::Unit),
        VariantShape::Tuple { fields } => {
            let fields = crate::field::assemble_descriptors(fields);
            quote!(::rostfrei_domain::ValueObjectVariantShapeDescriptor::Tuple { fields: #fields })
        }
        VariantShape::Struct { fields } => {
            let fields = crate::field::assemble_descriptors(fields);
            quote!(::rostfrei_domain::ValueObjectVariantShapeDescriptor::Struct { fields: #fields })
        }
    };
    quote!(::rostfrei_domain::ValueObjectVariantDescriptor { name: #name, shape: #shape })
}
