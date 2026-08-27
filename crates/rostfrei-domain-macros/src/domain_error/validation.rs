use syn::Result;

use crate::field::{Field, Role};

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes, fields: &[Field]) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    crate::helper::error_code::validate(&attributes.code)?;
    crate::helper::message::validate(&attributes.message)?;
    for field in fields {
        if attributes.json && matches!(field.name.value().as_str(), "code" | "message") {
            return Err(syn::Error::new_spanned(
                &field.name,
                "generated JSON reserves domain error fields `code` and `message`",
            ));
        }
        if matches!(field.role, Role::Entity) {
            return Err(syn::Error::new_spanned(
                &field.base,
                "DomainError cannot contain Entity fields",
            ));
        }
    }
    Ok(())
}
