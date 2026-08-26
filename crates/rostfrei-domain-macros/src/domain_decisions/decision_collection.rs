use std::collections::HashSet;

use syn::{TraitItem, TraitItemFn};

use super::decision::Decision;

pub fn collect(items: &mut [TraitItem]) -> syn::Result<Vec<Decision>> {
    if items.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain decision contract traits require at least one method",
        ));
    }

    let mut decisions = Vec::with_capacity(items.len());
    let mut ids = HashSet::new();
    for item in items {
        let TraitItem::Fn(method) = item else {
            return Err(syn::Error::new_spanned(
                item,
                "domain decision contract traits may only contain decision methods",
            ));
        };
        decisions.push(collect_method(method, &mut ids)?);
    }
    Ok(decisions)
}

fn collect_method(method: &mut TraitItemFn, ids: &mut HashSet<String>) -> syn::Result<Decision> {
    validate_method_name(method)?;
    validate_body(method)?;
    let types = super::signature::parse(&method.sig)?;
    let attribute = super::decision_attribute::extract(method)?;
    crate::helper::id::validate(&attribute.id)?;
    crate::helper::label::validate(&attribute.label)?;
    validate_unique_id(&attribute.id, ids)?;

    Ok(Decision {
        id: attribute.id,
        label: attribute.label,
        input: types.input,
        output: types.output,
    })
}

fn validate_method_name(method: &TraitItemFn) -> syn::Result<()> {
    let name = method.sig.ident.to_string();
    if matches!(
        name.as_str(),
        "__DOMAIN_DECISIONS" | "__DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE"
    ) {
        return Err(syn::Error::new_spanned(
            &method.sig.ident,
            format!("`{name}` is reserved by domain_decisions"),
        ));
    }
    Ok(())
}

fn validate_body(method: &TraitItemFn) -> syn::Result<()> {
    if let Some(default) = &method.default {
        return Err(syn::Error::new_spanned(
            default,
            "domain decision contract methods cannot have default bodies",
        ));
    }
    Ok(())
}

fn validate_unique_id(id: &syn::LitStr, ids: &mut HashSet<String>) -> syn::Result<()> {
    if ids.insert(id.value()) {
        Ok(())
    } else {
        Err(syn::Error::new(id.span(), "duplicate decision id"))
    }
}
