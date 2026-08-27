use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path, TypePath};

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    owner: &TypePath,
    value: &TypePath,
    semantic_scalar: Option<&TypePath>,
) -> TokenStream {
    let (scalar, semantic_scalar, assertion) = semantic_scalar.map_or_else(
        || {
            (
                crate::field::assemble_scalar(domain_path, value),
                quote!(None),
                TokenStream::new(),
            )
        },
        |provider| {
            (
                quote!(<#provider as #domain_path::SemanticScalar>::DESCRIPTOR.representation),
                quote!(Some(<#provider as #domain_path::SemanticScalar>::DESCRIPTOR)),
                quote! {
                    const _: () = {
                        fn assert_semantic_scalar<P, V>()
                        where
                            P: #domain_path::SemanticScalar<Value = V>,
                            V: 'static,
                        {}
                        let _ = assert_semantic_scalar::<#provider, #value>;
                    };
                },
            )
        },
    );
    quote! {
        impl #domain_path::DomainIdentityType for #name {
            type Owner = #owner;

            const SEMANTIC_SCALAR: ::core::option::Option<#domain_path::SemanticScalarDescriptor> =
                #semantic_scalar;

            const DESCRIPTOR: #domain_path::DomainIdentityDescriptor =
                #domain_path::DomainIdentityDescriptor {
                    id: #domain_path::DomainIdentityId {
                        owner: #domain_path::EntityId {
                            aggregate: <<#owner as #domain_path::EntityType>::Owner as #domain_path::AggregateType>::DESCRIPTOR.id,
                            local: <#owner as #domain_path::EntityType>::LOCAL_ID,
                        },
                    },
                    scalar: #scalar,
                };
        }

        #assertion

        impl #domain_path::ActionInputType<<#owner as #domain_path::EntityType>::Owner> for #name {
            const DESCRIPTOR: #domain_path::ActionInputDescriptor =
                #domain_path::ActionInputDescriptor::DomainIdentity(
                    <Self as #domain_path::DomainIdentityType>::DESCRIPTOR.id,
                );
        }

        impl #domain_path::QueryInputType<<#owner as #domain_path::EntityType>::Owner> for #name {
            const DESCRIPTOR: #domain_path::QueryInputDescriptor =
                #domain_path::QueryInputDescriptor::DomainIdentity(
                    <Self as #domain_path::DomainIdentityType>::DESCRIPTOR.id,
                );
        }

        impl #domain_path::QueryOutputType<<#owner as #domain_path::EntityType>::Owner> for #name {
            const DESCRIPTOR: #domain_path::QueryOutputDescriptor =
                #domain_path::QueryOutputDescriptor::DomainIdentity(
                    <Self as #domain_path::DomainIdentityType>::DESCRIPTOR.id,
                );
        }
    }
}
