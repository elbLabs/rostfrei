use std::collections::HashSet;

use super::action::Action;

pub fn validate_common(actions: &[Action]) -> syn::Result<()> {
    if actions.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain_actions requires at least one action",
        ));
    }
    let mut ids = HashSet::new();
    for action in actions {
        crate::helper::id::validate(&action.id)?;
        crate::helper::label::validate(&action.label)?;
        if !ids.insert(action.id.value()) {
            return Err(syn::Error::new(action.id.span(), "duplicate action id"));
        }
        validate_signature(action)?;
    }
    Ok(())
}

fn validate_signature(action: &Action) -> syn::Result<()> {
    let signature = &action.syntax;
    if signature.variadic.is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "actions cannot be variadic",
        ));
    }
    if signature.asyncness.is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "actions cannot be async",
        ));
    }
    if signature.unsafety.is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "actions cannot be unsafe",
        ));
    }
    if signature.abi.is_some() {
        return Err(syn::Error::new_spanned(
            signature,
            "actions cannot be extern",
        ));
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &signature.generics,
            "actions cannot have generic parameters or where clauses",
        ));
    }
    Ok(())
}
