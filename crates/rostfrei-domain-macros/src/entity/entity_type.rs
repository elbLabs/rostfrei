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
) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);
    quote! {
        impl #domain_path::EntityType for #name {
            const LOCAL_ID: &'static str = #id;
            const DESCRIPTOR: #domain_path::EntityDescriptor = {
                let id = #domain_path::EntityId {
                        aggregate: <<Self as #domain_path::EntityDefinition>::Owner as
                            #domain_path::AggregateType>::DESCRIPTOR.id,
                        local: #id,
                };
                #domain_path::EntityDescriptor {
                    id,
                    label: #label,
                    identity: #domain_path::DomainIdentityId { owner: id },
                    fields: #fields,
                }
            };
        }
    }
}
