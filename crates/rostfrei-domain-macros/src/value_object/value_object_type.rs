use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;
use super::ir::{Shape, Variant, VariantShape};

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    shape: &Shape,
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let owner = &attributes.owner;
    let actions = &attributes.actions;
    let invariants = &attributes.invariants;
    let shape = assemble_shape(domain_path, shape);
    quote! {
        impl #domain_path::ValueObjectType for #name {
            type Owner = #owner;

            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::ValueObjectDescriptor =
                #domain_path::ValueObjectDescriptor {
                    id: #domain_path::ValueObjectId {
                        owner: <#owner as #domain_path::ValueObjectOwnerType>::VALUE_OBJECT_OWNER_ID,
                        local: Self::LOCAL_ID,
                    },
                    label: #label,
                    shape: #shape,
                };
            const ACTION_CONTRACTS: &'static [&'static [#domain_path::ActionDescriptor]] = &[
                #(<Self as #actions>::__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE,)*
            ];
            const INVARIANT_CONTRACTS: &'static [&'static [#domain_path::InvariantDescriptor]] = &[
                #(<Self as #invariants>::__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE,)*
            ];
        }
    }
}

fn assemble_shape(domain_path: &Path, shape: &Shape) -> TokenStream {
    match shape {
        Shape::Struct { fields } => {
            let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);
            quote!(#domain_path::ValueObjectShapeDescriptor::Struct { fields: #fields })
        }
        Shape::Enum { variants } => {
            quote!(#domain_path::ValueObjectShapeDescriptor::Enum { variants: &[#(#variants),*] })
        }
        Shape::TaggedEnum { variants } => {
            let variants = variants
                .iter()
                .map(|variant| assemble_variant(domain_path, variant));
            quote!(#domain_path::ValueObjectShapeDescriptor::TaggedEnum { variants: &[#(#variants),*] })
        }
    }
}

fn assemble_variant(domain_path: &Path, variant: &Variant) -> TokenStream {
    let name = &variant.name;
    let shape = match &variant.shape {
        VariantShape::Unit => quote!(#domain_path::ValueObjectVariantShapeDescriptor::Unit),
        VariantShape::Tuple { fields } => {
            let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);
            quote!(#domain_path::ValueObjectVariantShapeDescriptor::Tuple { fields: #fields })
        }
        VariantShape::Struct { fields } => {
            let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);
            quote!(#domain_path::ValueObjectVariantShapeDescriptor::Struct { fields: #fields })
        }
    };
    quote!(#domain_path::ValueObjectVariantDescriptor { name: #name, shape: #shape })
}
