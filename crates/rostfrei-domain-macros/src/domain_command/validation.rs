use syn::Result;

use super::attributes::Attributes;
use crate::field::{Field, Role};

pub fn validate(attributes: &Attributes, fields: &[Field]) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    for field in fields {
        if matches!(field.role, Role::Entity) {
            return Err(syn::Error::new_spanned(
                &field.base,
                "DomainCommand cannot contain Entity fields",
            ));
        }
    }
    Ok(())
}
