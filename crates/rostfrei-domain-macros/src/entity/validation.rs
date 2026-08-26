use syn::Result;

use crate::field::{Field, Role};

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes, fields: &[Field]) -> Result<usize> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    let identities: Vec<_> = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| matches!(field.role, Role::Identity))
        .collect();
    if identities.len() != 1 {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "Entity requires exactly one identity field",
        ));
    }
    let (index, identity) = identities[0];
    if !identity.wrappers.is_empty() {
        return Err(syn::Error::new_spanned(
            &identity.base,
            "identity field must use a direct, non-wrapped type",
        ));
    }
    Ok(index)
}
