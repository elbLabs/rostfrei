use syn::{ItemTrait, Result, TraitItem};

use super::attributes::Attributes;

pub fn validate(attributes: &Attributes) -> Result<()> {
    crate::helper::id::validate(&attributes.id)?;
    crate::helper::label::validate(&attributes.label)
}

pub fn validate_trait(item: &ItemTrait) -> Result<()> {
    for item in &item.items {
        let ident = match item {
            TraitItem::Const(item) => Some(&item.ident),
            TraitItem::Fn(item) => Some(&item.sig.ident),
            TraitItem::Type(item) => Some(&item.ident),
            _ => None,
        };
        if let Some(ident) = ident
            && matches!(
                ident.to_string().as_str(),
                "LOCAL_ID" | "LABEL" | "DESCRIPTOR"
            )
        {
            return Err(syn::Error::new_spanned(
                ident,
                format!("reserved domain_invariant associated item `{ident}`"),
            ));
        }
    }
    Ok(())
}
