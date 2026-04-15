use syn::Result;

use super::attributes::Attributes;
use crate::field::{Field, Role};

pub fn validate(attributes: &Attributes, fields: &[Field]) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    if attributes.schema_version.base10_parse::<u32>()? == 0 {
        return Err(syn::Error::new_spanned(
            &attributes.schema_version,
            "schema_version must be greater than zero",
        ));
    }
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
