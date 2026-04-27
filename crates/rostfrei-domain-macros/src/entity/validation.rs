use syn::Result;

use crate::field::{Field, Role};

pub fn validate(fields: &[Field]) -> Result<&Field> {
    let mut identities = fields
        .iter()
        .filter(|field| matches!(field.role, Role::Identity));
    let Some(identity) = identities.next() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "Entity requires exactly one identity field",
        ));
    };
    if identities.next().is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "Entity requires exactly one identity field",
        ));
    }
    if !identity.wrappers.is_empty() {
        return Err(syn::Error::new_spanned(
            &identity.base,
            "identity field must use a direct, non-wrapped type",
        ));
    }
    Ok(identity)
}
