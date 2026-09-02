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

    }
}
