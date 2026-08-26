use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

use crate::field::Field;

use super::{attributes::Attributes, entity_type, lifecycle, owner_traits};

pub fn assemble(
    name: &Ident,
    attributes: &Attributes,
    fields: &[Field],
    identity: usize,
) -> TokenStream {
    let entity_type = entity_type::assemble(name, attributes, fields, identity);
    let field_assertions = crate::field::assemble_assertions(name, Some(&attributes.owner), fields);
    let owner_traits = owner_traits::assemble(name, attributes);
    let lifecycle = lifecycle::assemble(name, attributes);
    quote! {
        #entity_type
        #field_assertions
        #owner_traits
        #lifecycle
    }
}
