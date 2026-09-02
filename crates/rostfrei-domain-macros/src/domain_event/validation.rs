use syn::Result;

use crate::field::{Field, Role};

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes, fields: &[Field]) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    if let Some(schema_version) = &attributes.schema_version
        && schema_version.base10_parse::<u32>()? == 0
    {
        return Err(syn::Error::new_spanned(
            schema_version,
            "domain event schema version must be greater than zero",
        ));
    }
    reject_entities(fields)
}

fn reject_entities(fields: &[Field]) -> Result<()> {
    for field in fields {
        if matches!(field.role, Role::Entity) {
            return Err(syn::Error::new_spanned(
                &field.base,
                "DomainEvent cannot contain Entity fields",
            ));
        }
    }
    Ok(())
}
