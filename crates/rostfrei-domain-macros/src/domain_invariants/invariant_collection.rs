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
        match item {
            TraitItem::Fn(method) => {
                if let Some(invariant) = collect_method(method, &mut ids)? {
                    invariants.push(invariant);
                }
            }
            item if has_invariant_attribute(item) => {
                return Err(syn::Error::new_spanned(
                    item,
                    "invariant may only be applied to trait methods",
                ));
            }
            _ => {}
        }
    }
    if invariants.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain invariant contract traits require at least one annotated checker",
        ));
    }
    Ok(invariants)
}

fn collect_method(
    method: &mut TraitItemFn,
    ids: &mut HashSet<String>,
) -> syn::Result<Option<Invariant>> {
    let Some(attribute) = super::invariant_attribute::extract(method)? else {
        return Ok(None);
    };
    crate::helper::id::validate(&attribute.id)?;
    crate::helper::label::validate(&attribute.label)?;
    validate_unique_id(&attribute.id, ids)?;

    Ok(Some(Invariant {
        id: attribute.id,
        label: attribute.label,
    }))
}

fn has_invariant_attribute(item: &TraitItem) -> bool {
    let attributes = match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        _ => return false,
    };
    attributes
        .iter()
        .any(|attribute| attribute.path().is_ident("invariant"))
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
    (identifier == "__DOMAIN_INVARIANTS").then_some("__DOMAIN_INVARIANTS")
}
