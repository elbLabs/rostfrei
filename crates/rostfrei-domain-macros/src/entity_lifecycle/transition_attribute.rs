use syn::{Attribute, Ident};

use super::action_reference;
use super::ir::Transition;

pub fn parse(attribute: &Attribute) -> syn::Result<Transition> {
    let mut action = None;
    let mut target = None;

    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("action") {
            if action.is_some() {
                return Err(meta.error("duplicate transition action"));
            }
            action = Some(action_reference::parse(meta.value()?)?);
            return Ok(());
        }
        if meta.path.is_ident("to") {
            if target.is_some() {
                return Err(meta.error("duplicate transition target"));
            }
            target = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        Err(meta.error("unsupported transition metadata"))
    })?;

    Ok(Transition {
        action: action
            .ok_or_else(|| syn::Error::new_spanned(attribute, "missing transition action"))?,
        target: target
            .ok_or_else(|| syn::Error::new_spanned(attribute, "missing transition target"))?,
    })
}
