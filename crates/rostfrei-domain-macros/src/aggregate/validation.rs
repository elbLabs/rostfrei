use syn::Result;

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)
}
