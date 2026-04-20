use std::collections::HashSet;

use syn::{ImplItem, ImplItemFn};

use super::decision::Decision;

pub fn collect(items: &mut [ImplItem]) -> syn::Result<Vec<Decision>> {
    let mut decisions = Vec::new();
    let mut ids = HashSet::new();
    for item in items {
        let ImplItem::Fn(method) = item else {
            continue;
        };
        if let Some(decision) = collect_method(method, &mut ids)? {
            decisions.push(decision);
        }
    }
    if decisions.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain_decisions requires at least one #[decision(...)] method",
        ));
    }
    Ok(decisions)
}

fn collect_method(
    method: &mut ImplItemFn,
    ids: &mut HashSet<String>,
) -> syn::Result<Option<Decision>> {
    let Some(attribute) = super::decision_attribute::extract(method)? else {
        return Ok(None);
    };
    let types = super::signature::parse(&method.sig)?;
    crate::helper::id::validate(&attribute.id)?;
    crate::helper::label::validate(&attribute.label)?;
    validate_unique_id(&attribute.id, ids)?;
    Ok(Some(Decision {
        name: method.sig.ident.clone(),
        visibility: method.vis.clone(),
        cfg_attributes: super::cfg_attributes::collect(&method.attrs),
        id: attribute.id,
        label: attribute.label,
        parameters: types.parameters,
        return_type: types.return_type,
    }))
}

fn validate_unique_id(id: &syn::LitStr, ids: &mut HashSet<String>) -> syn::Result<()> {
    if ids.insert(id.value()) {
        Ok(())
    } else {
        Err(syn::Error::new(id.span(), "duplicate decision id"))
    }
}
