use syn::Result;

use crate::field::{Field, Role};

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes, fields: &[Field]) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    crate::helper::error_code::validate(&attributes.code)?;
    crate::helper::message::validate(&attributes.message)?;
    for field in fields {
        if matches!(field.role, Role::Entity) {
            return Err(syn::Error::new_spanned(
                &field.base,
                "DomainError cannot contain Entity fields",
            ));
        }
    }
    Ok(())
}
