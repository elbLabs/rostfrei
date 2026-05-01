use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use super::attributes::Attributes;

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    let id = &attributes.id;
    let label = &attributes.label;

    quote! {
        impl #domain_path::DomainServiceType for #name {
            const DESCRIPTOR: #domain_path::DomainServiceDescriptor =
                #domain_path::DomainServiceDescriptor {
                    id: #domain_path::DomainServiceId {
                        context: <<Self as #domain_path::DomainServiceDefinition>::Context as
                            #domain_path::BoundedContextType>::DESCRIPTOR.id,
                        local: #id,
                    },
                    label: #label,
                };
        }
    }
}
