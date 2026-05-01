use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;
    quote! {
        impl #domain_path::AggregateType for #name {
            const DESCRIPTOR: #domain_path::AggregateDescriptor = {
                let id = #domain_path::AggregateId {
                        context: <<Self as #domain_path::AggregateDefinition>::Context as
                            #domain_path::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                };
                #domain_path::AggregateDescriptor {
                    id,
                    label: #label,
                    root: #domain_path::EntityId {
                        aggregate: id,
                        local: <<Self as #domain_path::AggregateDefinition>::Root as
                            #domain_path::EntityType>::LOCAL_ID,
                    },
                }
            };
        }
    }
}
