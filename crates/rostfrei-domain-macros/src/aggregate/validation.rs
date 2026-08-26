use syn::Result;

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)?;
    if attributes.events.as_ref().is_some_and(Vec::is_empty) {
        return Err(syn::Error::new_spanned(
            &attributes.root,
            "an executable aggregate must register at least one event",
        ));
    }
    Ok(())
}
