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
                quote!(<#provider as ::rostfrei_domain::SemanticScalar>::DESCRIPTOR.representation),
                quote!(Some(<#provider as ::rostfrei_domain::SemanticScalar>::DESCRIPTOR)),
                quote! {
                    const _: () = {
                        fn assert_semantic_scalar<P, V>()
                        where
                            P: ::rostfrei_domain::SemanticScalar<Value = V>,
                            V: 'static,
                        {}
                        let _ = assert_semantic_scalar::<#provider, #value>;
                    };
                },
            )
        },
    );
    quote! {
        impl ::rostfrei_domain::DomainIdentityType for #name {
            type Owner = #owner;

            const SEMANTIC_SCALAR: ::core::option::Option<::rostfrei_domain::SemanticScalarDescriptor> =
                #semantic_scalar;

            const DESCRIPTOR: ::rostfrei_domain::DomainIdentityDescriptor =
                ::rostfrei_domain::DomainIdentityDescriptor {
                    id: ::rostfrei_domain::DomainIdentityId {
                        owner: ::rostfrei_domain::EntityId {
                            aggregate: <<#owner as ::rostfrei_domain::EntityType>::Owner as ::rostfrei_domain::AggregateType>::DESCRIPTOR.id,
                            local: <#owner as ::rostfrei_domain::EntityType>::LOCAL_ID,
                        },
                    },
                    scalar: #scalar,
                };
        }

        #assertion

        impl ::rostfrei_domain::QueryInputType<<#owner as ::rostfrei_domain::EntityType>::Owner> for #name {
            const DESCRIPTOR: ::rostfrei_domain::QueryInputDescriptor =
                ::rostfrei_domain::QueryInputDescriptor::DomainIdentity(
                    <Self as ::rostfrei_domain::DomainIdentityType>::DESCRIPTOR.id,
                );
        }

        impl ::rostfrei_domain::QueryOutputType<<#owner as ::rostfrei_domain::EntityType>::Owner> for #name {
            const DESCRIPTOR: ::rostfrei_domain::QueryOutputDescriptor =
                ::rostfrei_domain::QueryOutputDescriptor::DomainIdentity(
                    <Self as ::rostfrei_domain::DomainIdentityType>::DESCRIPTOR.id,
                );
        }
    }
}
