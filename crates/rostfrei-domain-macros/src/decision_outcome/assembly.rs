use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path, Type};

use super::ir::{Outcome, Shape, ValueField};

pub fn assemble(domain_path: &Path, name: &Ident, outcomes: &[Outcome]) -> TokenStream {
    let descriptors = outcomes
        .iter()
        .map(|outcome| assemble_descriptor(domain_path, outcome));
    let assertions = outcomes
        .iter()
        .map(|outcome| assemble_assertions(domain_path, outcome));
    quote! {
        impl #domain_path::DecisionOutcomeType for #name {
            const OUTCOMES: &'static [#domain_path::DecisionOutcomeDescriptor] = &[
                #(#descriptors),*
            ];
        }

        #(#assertions)*
    }
}

fn assemble_descriptor(domain_path: &Path, outcome: &Outcome) -> TokenStream {
    let cfg_attributes = &outcome.cfg_attributes;
    let local_id = &outcome.local_id;
    let label = &outcome.label;
    let shape = assemble_shape(domain_path, &outcome.shape);
    quote! {
        #(#cfg_attributes)*
        #domain_path::DecisionOutcomeDescriptor {
            local_id: #local_id,
            label: #label,
            shape: #shape,
        }
    }
}

fn assemble_shape(domain_path: &Path, shape: &Shape) -> TokenStream {
    match shape {
        Shape::Unit => quote!(#domain_path::DecisionOutcomeShapeDescriptor::Unit),
        Shape::Tuple { fields } => {
            let fields = fields.iter().map(|field| {
                let cfg_attributes = &field.cfg_attributes;
                let value = value_descriptor(domain_path, &field.ty);
                quote! {
                    #(#cfg_attributes)*
                    #value
                }
            });
            quote!(#domain_path::DecisionOutcomeShapeDescriptor::Tuple {
                fields: &[#(#fields),*],
            })
        }
        Shape::Struct { fields } => {
            let fields = fields.iter().map(|field| {
                let cfg_attributes = &field.value.cfg_attributes;
                let name = &field.name;
                let value = value_descriptor(domain_path, &field.value.ty);
                quote! {
                    #(#cfg_attributes)*
                    #domain_path::DecisionOutcomeNamedFieldDescriptor {
                        name: #name,
                        value: #value,
                    }
                }
            });
            quote!(#domain_path::DecisionOutcomeShapeDescriptor::Struct {
                fields: &[#(#fields),*],
            })
        }
    }
}

fn value_descriptor(domain_path: &Path, ty: &Type) -> TokenStream {
    quote!(<#ty as #domain_path::DecisionOutcomeValueType>::DESCRIPTOR)
}

fn assemble_assertions(domain_path: &Path, outcome: &Outcome) -> TokenStream {
    let assertions: Vec<_> = match &outcome.shape {
        Shape::Unit => Vec::new(),
        Shape::Tuple { fields } => fields
            .iter()
            .map(|field| assemble_value_assertion(domain_path, outcome, field))
            .collect(),
        Shape::Struct { fields } => fields
            .iter()
            .map(|field| assemble_value_assertion(domain_path, outcome, &field.value))
            .collect(),
    };
    quote!(#(#assertions)*)
}

fn assemble_value_assertion(
    domain_path: &Path,
    outcome: &Outcome,
    field: &ValueField,
) -> TokenStream {
    let outcome_cfg_attributes = &outcome.cfg_attributes;
    let field_cfg_attributes = &field.cfg_attributes;
    let ty = &field.ty;
    quote! {
        #(#outcome_cfg_attributes)*
        #(#field_cfg_attributes)*
        const _: () = {
            fn assert_value_type<T: #domain_path::DecisionOutcomeValueType>() {}
            fn check() {
                assert_value_type::<#ty>();
            }
        };
    }
}
