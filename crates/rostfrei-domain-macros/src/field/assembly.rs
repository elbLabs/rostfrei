use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, TypePath};

use super::ir::{Field, Role, Scalar, Wrapper};

pub fn assemble_descriptors(fields: &[Field]) -> TokenStream {
    let fields = fields.iter().map(|field| {
        let name = &field.name;
        let wrappers = field.wrappers.iter().map(|wrapper| match wrapper {
            Wrapper::List => quote!(::rostfrei_domain::FieldWrapper::List),
            Wrapper::Optional => quote!(::rostfrei_domain::FieldWrapper::Optional),
        });
        let kind = assemble_kind(field);
        quote!(::rostfrei_domain::FieldDescriptor {
            name: #name,
            value: ::rostfrei_domain::FieldValue { kind: #kind, wrappers: &[#(#wrappers),*] },
        })
    });
    quote!(&[#(#fields),*])
}

pub fn assemble_assertions(
    container: &Ident,
    owner: Option<&TypePath>,
    fields: &[Field],
) -> TokenStream {
    let assertions = fields.iter().map(|field| {
        let base = &field.base;
        match &field.role {
            Role::Identity | Role::AggregateReference(_) => quote!(assert_identity::<#base>();),
            Role::Entity => {
                let owner = owner.unwrap();
                quote!(assert_entity::<#base, #owner>();)
            }
            Role::ValueObject => quote!(assert_value_object::<#base>();),
            Role::SemanticScalar(provider) => {
                quote!(assert_semantic_scalar::<#provider, #base>();)
            }
            Role::Scalar(_) => quote!(),
        }
    });
    quote! {
        const _: () = {
            fn assert_identity<T: ::rostfrei_domain::DomainIdentityType>() {}
            fn assert_entity<T, O>() where T: ::rostfrei_domain::EntityType<Owner = O>, O: ::rostfrei_domain::AggregateType {}
            fn assert_value_object<T: ::rostfrei_domain::ValueObjectType>() {}
            fn assert_semantic_scalar<P, V>()
            where
                P: ::rostfrei_domain::SemanticScalar<Value = V>,
                V: 'static,
            {}
            fn assert_container<T: 'static>() {}
            let _ = assert_container::<#container>;
            fn check() {
                #(#assertions)*
            }
        };
    }
}

fn assemble_kind(field: &Field) -> TokenStream {
    let base = &field.base;
    match &field.role {
        Role::Identity => quote!(::rostfrei_domain::FieldKind::DomainIdentity(
            <#base as ::rostfrei_domain::DomainIdentityType>::DESCRIPTOR.id,
        )),
        Role::Entity => {
            quote!(::rostfrei_domain::FieldKind::Entity(::rostfrei_domain::EntityId {
                aggregate: <<#base as ::rostfrei_domain::EntityType>::Owner as ::rostfrei_domain::AggregateType>::DESCRIPTOR.id,
                local: <#base as ::rostfrei_domain::EntityType>::LOCAL_ID,
            }))
        }
        Role::ValueObject => {
            quote!(::rostfrei_domain::FieldKind::ValueObject(::rostfrei_domain::ValueObjectId {
                owner: <<#base as ::rostfrei_domain::ValueObjectType>::Owner as ::rostfrei_domain::ValueObjectOwnerType>::VALUE_OBJECT_OWNER_ID,
                local: <#base as ::rostfrei_domain::ValueObjectType>::LOCAL_ID,
            }))
        }
        Role::AggregateReference(target) => {
            quote!(::rostfrei_domain::FieldKind::AggregateReference(<#target as ::rostfrei_domain::AggregateType>::DESCRIPTOR.id))
        }
        Role::SemanticScalar(provider) => quote!(::rostfrei_domain::FieldKind::SemanticScalar(
            <#provider as ::rostfrei_domain::SemanticScalar>::DESCRIPTOR,
        )),
        Role::Scalar(scalar) => {
            let scalar = scalar_tokens(scalar);
            quote!(::rostfrei_domain::FieldKind::Scalar(#scalar))
        }
    }
}

pub fn assemble_scalar(path: &syn::TypePath) -> TokenStream {
    scalar_tokens(&super::scalar::classify(path).unwrap())
}

fn scalar_tokens(scalar: &Scalar) -> TokenStream {
    let variant = match scalar {
        Scalar::Bool => "Bool",
        Scalar::String => "String",
        Scalar::Char => "Char",
        Scalar::F32 => "F32",
        Scalar::F64 => "F64",
        Scalar::I8 => "I8",
        Scalar::I16 => "I16",
        Scalar::I32 => "I32",
        Scalar::I64 => "I64",
        Scalar::I128 => "I128",
        Scalar::Isize => "Isize",
        Scalar::U8 => "U8",
        Scalar::U16 => "U16",
        Scalar::U32 => "U32",
        Scalar::U64 => "U64",
        Scalar::U128 => "U128",
        Scalar::Usize => "Usize",
    };
    let ident = Ident::new(variant, proc_macro2::Span::call_site());
    quote!(::rostfrei_domain::ScalarType::#ident)
}
