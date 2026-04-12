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
    let schema_version = &attributes.schema_version;
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);

    quote! {
        impl #domain_path::DomainEventDefinitionType for #name {
            const DEFINITION: #domain_path::DomainEventDefinition =
                #domain_path::DomainEventDefinition {
                    id: #id,
                    label: #label,
                    schema_version: #schema_version,
                    fields: #fields,
                };
        }

        #assertions
    }
}
