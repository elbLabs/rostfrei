use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path};

use crate::field::Field;

use super::{attributes::Attributes, entity_type, identity as identity_binding, owner_traits};

pub fn assemble(
    domain_path: &Path,
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
    identity: &Field,
) -> TokenStream {
    let entity_type = entity_type::assemble(domain_path, name, attributes, fields, identity);
    let identity_binding = identity_binding::assemble(domain_path, name, identity);
    let owner: syn::TypePath = syn::parse_quote!(
        <#name as #domain_path::EntityDefinition>::Owner
    );
    let field_assertions =
        crate::field::assemble_assertions_with_path(domain_path, name, Some(&owner), fields);
    let owner_traits = owner_traits::assemble(domain_path, name);
    quote! {
        #entity_type
        #identity_binding
        #field_assertions
        #owner_traits
    }
}
