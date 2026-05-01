use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use crate::field::Field;

use super::{attributes::Attributes, entity_type};

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
) -> TokenStream {
    let entity_type = entity_type::assemble(domain_path, name, attributes, fields);
    let owner: syn::TypePath = syn::parse_quote!(
        <#name as #domain_path::EntityDefinition>::Owner
    );
    let field_assertions =
        crate::field::assemble_assertions_with_path(domain_path, name, Some(&owner), fields);
    quote! {
        #entity_type
        #field_assertions
    }
}
