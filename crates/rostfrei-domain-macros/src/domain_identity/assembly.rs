use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, TypePath};

pub fn assemble(
    name: &Ident,
    owner: &TypePath,
    value: &TypePath,
    semantic_scalar: Option<&TypePath>,
) -> TokenStream {
    let (scalar, semantic_scalar, assertion) = semantic_scalar.map_or_else(
        || {
            (
                crate::field::assemble_scalar(value),
                quote!(None),
                TokenStream::new(),
            )
        },
        |provider| {
            (
                quote!(<#provider as ::domain::SemanticScalar>::DESCRIPTOR.representation),
                quote!(Some(<#provider as ::domain::SemanticScalar>::DESCRIPTOR)),
                quote! {
                    const _: () = {
                        fn assert_semantic_scalar<P, V>()
                        where
                            P: ::domain::SemanticScalar<Value = V>,
                            V: 'static,
                        {}
                        let _ = assert_semantic_scalar::<#provider, #value>;
                    };
                },
            )
        },
    );
    quote! {
        impl ::domain::DomainIdentityType for #name {
            type Owner = #owner;

            const SEMANTIC_SCALAR: ::core::option::Option<::domain::SemanticScalarDescriptor> =
                #semantic_scalar;

            const DESCRIPTOR: ::domain::DomainIdentityDescriptor =
                ::domain::DomainIdentityDescriptor {
                    id: ::domain::DomainIdentityId {
                        owner: ::domain::EntityId {
                            aggregate: <<#owner as ::domain::EntityType>::Owner as ::domain::AggregateType>::DESCRIPTOR.id,
                            local: <#owner as ::domain::EntityType>::LOCAL_ID,
                        },
                    },
                    scalar: #scalar,
                };
        }

        #assertion

        impl ::domain::QueryInputType<<#owner as ::domain::EntityType>::Owner> for #name {
            const DESCRIPTOR: ::domain::QueryInputDescriptor =
                ::domain::QueryInputDescriptor::DomainIdentity(
                    <Self as ::domain::DomainIdentityType>::DESCRIPTOR.id,
                );
        }

        impl ::domain::QueryOutputType<<#owner as ::domain::EntityType>::Owner> for #name {
            const DESCRIPTOR: ::domain::QueryOutputDescriptor =
                ::domain::QueryOutputDescriptor::DomainIdentity(
                    <Self as ::domain::DomainIdentityType>::DESCRIPTOR.id,
                );
        }
    }
}
