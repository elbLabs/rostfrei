use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use crate::field::Field;

use super::attributes::Attributes;

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
    identity: &Field,
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let identity_name = &identity.name;
    let identity_type = &identity.base;
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);
    quote! {
        impl #domain_path::EntityType for #name {
            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::EntityDescriptor =
                #domain_path::EntityDescriptor {
                    id: #domain_path::EntityId {
                        aggregate: <<Self as #domain_path::EntityDefinition>::Owner as
                            #domain_path::AggregateType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                    identity: #domain_path::IdentityDescriptor {
                        field: #identity_name,
                        identity: <<Self as #domain_path::EntityDefinition>::Identity as
                            #domain_path::DomainIdentityType>::DESCRIPTOR.id,
                    },
                    fields: #fields,
                };
        }

        const _: () = {
            fn assert_identity_field<Identity>()
            where
                #name: #domain_path::EntityDefinition<Identity = Identity>,
                Identity: #domain_path::DomainIdentityType<Owner = #name>,
            {
            }
            let _ = assert_identity_field::<#identity_type>;
        };
    }
}
