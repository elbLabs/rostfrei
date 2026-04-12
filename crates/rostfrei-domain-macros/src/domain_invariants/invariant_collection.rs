use std::collections::HashSet;

use syn::{TraitItem, TraitItemFn};

use super::invariant::Invariant;

pub fn collect(items: &mut [TraitItem]) -> syn::Result<Vec<Invariant>> {
    let mut invariants = Vec::with_capacity(items.len());
    let mut ids = HashSet::new();
    for item in items {
        if let Some(name) = reserved_name(item) {
            return Err(syn::Error::new_spanned(
                item,
                format!("`{name}` is reserved by domain_invariants"),
            ));
        }
        let TraitItem::Fn(method) = item else {
            return Err(syn::Error::new_spanned(
                item,
                "domain invariant contract traits may only contain methods",
            ));
        };
        invariants.push(collect_method(method, &mut ids)?);
    }
    if invariants.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain invariant contract traits require at least one annotated checker",
        ));
    }
    Ok(invariants)
}

fn collect_method(method: &mut TraitItemFn, ids: &mut HashSet<String>) -> syn::Result<Invariant> {
    let attribute = super::invariant_attribute::extract(method)?;
    validate_body(method)?;
    super::signature::validate(&method.sig)?;
    crate::helper::id::validate(&attribute.id)?;
    crate::helper::label::validate(&attribute.label)?;
    validate_unique_id(&attribute.id, ids)?;

    Ok(Invariant {
        id: attribute.id,
        label: attribute.label,
        method: method.sig.ident.clone(),
    })
}

fn validate_body(method: &TraitItemFn) -> syn::Result<()> {
    if let Some(default) = &method.default {
        return Err(syn::Error::new_spanned(
            default,
            "domain invariant checker methods cannot have default bodies",
        ));
    }
    Ok(())
}

fn validate_unique_id(id: &syn::LitStr, ids: &mut HashSet<String>) -> syn::Result<()> {
    if ids.insert(id.value()) {
        Ok(())
    } else {
        Err(syn::Error::new(id.span(), "duplicate invariant id"))
    }
}

fn reserved_name(item: &TraitItem) -> Option<&'static str> {
    let identifier = match item {
        TraitItem::Const(item) => &item.ident,
        TraitItem::Fn(item) => &item.sig.ident,
        TraitItem::Type(item) => &item.ident,
        _ => return None,
    };
    [
        "__DOMAIN_INVARIANTS",
        "__DOMAIN_INVARIANTS_TRAIT_REQUIRES_DOMAIN_INVARIANTS_ATTRIBUTE",
        "__DOMAIN_INVARIANTS_APPEND_VIOLATIONS",
    ]
    .into_iter()
    .find(|name| identifier == name)
}
