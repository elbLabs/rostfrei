use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use crate::field::Field;

use super::{attributes::Attributes, entity_type, lifecycle, owner_traits};

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
    identity: &Field,
) -> TokenStream {
    let entity_type = entity_type::assemble(domain_path, name, attributes, fields, identity);
    let field_assertions = crate::field::assemble_assertions_with_path(
        domain_path,
        name,
        Some(&attributes.owner),
        fields,
    );
    let owner_traits = owner_traits::assemble(domain_path, name, attributes);
    let lifecycle = lifecycle::assemble(domain_path, name, attributes);
    quote! {
        #entity_type
        #field_assertions
        #owner_traits
        #lifecycle
    }
}
