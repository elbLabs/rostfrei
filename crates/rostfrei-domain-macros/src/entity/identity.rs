use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use crate::field::Field;

pub fn assemble(domain_path: &Path, entity: &Ident, identity: &Field) -> TokenStream {
    let identity = &identity.base;

    quote! {
        impl #domain_path::__private::DomainIdentityType for #identity {
            type Owner = #entity;

            const DESCRIPTOR: #domain_path::__private::DomainIdentityDescriptor =
                #domain_path::__private::DomainIdentityDescriptor {
                    id: #domain_path::DomainIdentityId {
                        owner: #domain_path::EntityId {
                            aggregate: <<#entity as #domain_path::EntityDefinition>::Owner as
                                #domain_path::AggregateType>::DESCRIPTOR.id,
                            local: <#entity as #domain_path::EntityType>::LOCAL_ID,
                        },
                    },
                };
        }

        impl #domain_path::ActionInputType<
            <#entity as #domain_path::EntityDefinition>::Owner
        > for #identity {
            const DESCRIPTOR: #domain_path::ActionInputDescriptor =
                #domain_path::ActionInputDescriptor::DomainIdentity(
                    <Self as #domain_path::__private::DomainIdentityType>::DESCRIPTOR.id,
                );
        }

        impl #domain_path::QueryInputType<
            <#entity as #domain_path::EntityDefinition>::Owner
        > for #identity {
            const DESCRIPTOR: #domain_path::QueryInputDescriptor =
                #domain_path::QueryInputDescriptor::DomainIdentity(
                    <Self as #domain_path::__private::DomainIdentityType>::DESCRIPTOR.id,
                );
        }

        impl #domain_path::QueryOutputType<
            <#entity as #domain_path::EntityDefinition>::Owner
        > for #identity {
            const DESCRIPTOR: #domain_path::QueryOutputDescriptor =
                #domain_path::QueryOutputDescriptor::DomainIdentity(
                    <Self as #domain_path::__private::DomainIdentityType>::DESCRIPTOR.id,
                );
        }
    }
}
