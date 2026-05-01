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
    let schema_version = attributes.schema_version.as_ref().and_then(|version| {
        version
            .base10_parse::<u32>()
            .ok()
            .filter(|version| *version > 1)
            .map(|_| quote!(const SCHEMA_VERSION: u32 = #version;))
    });
    let assertions = crate::field::assemble_assertions_with_path(domain_path, name, None, fields);
    let fields = crate::field::assemble_descriptors_with_path(domain_path, fields);

    quote! {
        impl #domain_path::DomainEvent for #name {
            const LOCAL_ID: &'static str = #id;
            const LABEL: &'static str = #label;
            const FIELDS: &'static [#domain_path::FieldDescriptor] = #fields;
            #schema_version
        }

        #assertions
    }
}
