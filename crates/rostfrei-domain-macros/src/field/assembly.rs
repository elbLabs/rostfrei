use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path, TypePath};

use super::ir::{Field, Role, Scalar, Wrapper};

pub fn assemble_descriptors_with_path(domain_path: &Path, fields: &[Field]) -> TokenStream {
    let fields = fields.iter().map(|field| {
        let name = &field.name;
        let wrappers = field.wrappers.iter().map(|wrapper| match wrapper {
            Wrapper::List => quote!(#domain_path::FieldWrapper::List),
            Wrapper::Optional => quote!(#domain_path::FieldWrapper::Optional),
        });
        let kind = assemble_kind(domain_path, field);
        quote!(#domain_path::FieldDescriptor {
            name: #name,
            value: #domain_path::FieldValue { kind: #kind, wrappers: &[#(#wrappers),*] },
        })
    });
    quote!(&[#(#fields),*])
}

pub fn assemble_assertions_with_path(
    domain_path: &Path,
    container: &Ident,
    owner: Option<&TypePath>,
    fields: &[Field],
) -> TokenStream {
    let assertions = match fields
        .iter()
        .map(|field| assemble_assertion(field, owner))
        .collect::<syn::Result<Vec<_>>>()
    {
        Ok(assertions) => assertions,
        Err(error) => return error.into_compile_error(),
    };
    quote! {
        const _: () = {
            fn assert_identity<T: #domain_path::DomainIdentityType>() {}
            fn assert_entity<T, O>() where T: #domain_path::EntityDefinition<Owner = O>, O: #domain_path::AggregateType {}
            fn assert_value_object<T: #domain_path::ValueObjectType>() {}
            fn assert_semantic_scalar<P, V>()
            where
                P: #domain_path::SemanticScalar<Value = V>,
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

fn assemble_assertion(field: &Field, owner: Option<&TypePath>) -> syn::Result<TokenStream> {
    let base = &field.base;
    let assertion = match &field.role {
        Role::Identity | Role::AggregateReference(_) => quote!(assert_identity::<#base>();),
        Role::Entity => {
            let owner = owner.ok_or_else(|| {
                syn::Error::new_spanned(
                    base,
                    "Entity field owner must be validated before assembly",
                )
            })?;
            quote!(assert_entity::<#base, #owner>();)
        }
        Role::ValueObject => quote!(assert_value_object::<#base>();),
        Role::SemanticScalar(provider) => {
            quote!(assert_semantic_scalar::<#provider, #base>();)
        }
        Role::Scalar(_) => quote!(),
    };
    Ok(assertion)
}

fn assemble_kind(domain_path: &Path, field: &Field) -> TokenStream {
    let base = &field.base;
    match &field.role {
        Role::Identity => quote!(#domain_path::FieldKind::DomainIdentity(
            <#base as #domain_path::DomainIdentityType>::DESCRIPTOR.id,
        )),
        Role::Entity => {
            quote!(#domain_path::FieldKind::Entity(#domain_path::EntityId {
                aggregate: <<#base as #domain_path::EntityDefinition>::Owner as #domain_path::AggregateType>::DESCRIPTOR.id,
                local: <#base as #domain_path::EntityType>::LOCAL_ID,
            }))
        }
        Role::ValueObject => {
            quote!(#domain_path::FieldKind::ValueObject(#domain_path::ValueObjectId {
                owner: <<#base as #domain_path::ValueObjectType>::Owner as #domain_path::ValueObjectOwnerType>::VALUE_OBJECT_OWNER_ID,
                local: <#base as #domain_path::ValueObjectType>::LOCAL_ID,
            }))
        }
        Role::AggregateReference(target) => {
            quote!(#domain_path::FieldKind::AggregateReference(<#target as #domain_path::AggregateType>::DESCRIPTOR.id))
        }
        Role::SemanticScalar(provider) => quote!(#domain_path::FieldKind::SemanticScalar(
            <#provider as #domain_path::SemanticScalar>::DESCRIPTOR,
        )),
        Role::Scalar(scalar) => {
            let scalar = scalar_tokens(domain_path, scalar);
            quote!(#domain_path::FieldKind::Scalar(#scalar))
        }
    }
}

pub fn assemble_scalar(domain_path: &Path, path: &syn::TypePath) -> TokenStream {
    match validated_scalar(domain_path, path) {
        Ok(tokens) => tokens,
        Err(error) => error.into_compile_error(),
    }
}

fn validated_scalar(domain_path: &Path, path: &syn::TypePath) -> syn::Result<TokenStream> {
    let scalar = super::scalar::classify(path).ok_or_else(|| {
        syn::Error::new_spanned(
            path,
            "DomainIdentity field must be a supported canonical scalar",
        )
    })?;
    Ok(scalar_tokens(domain_path, &scalar))
}

fn scalar_tokens(domain_path: &Path, scalar: &Scalar) -> TokenStream {
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
    quote!(#domain_path::ScalarType::#ident)
}
