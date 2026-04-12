use syn::{LitStr, TraitItem};

use super::action::Action;

pub fn extract(items: &mut [TraitItem]) -> syn::Result<Vec<Action>> {
    if items.is_empty() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "domain action contract traits require at least one method",
        ));
    }

    let mut actions = Vec::with_capacity(items.len());
    for item in items {
        if let Some(name) = reserved_name(item) {
            return Err(syn::Error::new_spanned(
                item,
                format!("`{name}` is reserved by domain_actions"),
            ));
        }
        let TraitItem::Fn(method) = item else {
            return Err(syn::Error::new_spanned(
                item,
                "domain action contract traits may only contain action methods",
            ));
        };
        let positions: Vec<_> = method
            .attrs
            .iter()
            .enumerate()
            .filter(|(_, attr)| attr.path().is_ident("action"))
            .map(|(position, _)| position)
            .collect();
        if positions.is_empty() {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                "domain action contract methods require exactly one action attribute",
            ));
        }
        if positions.len() > 1 {
            return Err(syn::Error::new_spanned(
                &method.attrs[positions[1]],
                "duplicate action attribute",
            ));
        }
        let attribute = method.attrs.remove(positions[0]);
        let (id, label) = parse(&attribute)?;
        actions.push(Action {
            id,
            label,
            syntax: method.sig.clone(),
            signature: None,
        });
    }
    Ok(actions)
}

fn parse(attribute: &syn::Attribute) -> syn::Result<(LitStr, LitStr)> {
    let mut id = None;
    let mut label = None;
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            if id.is_some() {
                return Err(meta.error("duplicate id"));
            }
            id = Some(meta.value()?.parse::<LitStr>()?);
            return Ok(());
        }
        if meta.path.is_ident("label") {
            if label.is_some() {
                return Err(meta.error("duplicate label"));
            }
            label = Some(meta.value()?.parse::<LitStr>()?);
            return Ok(());
        }
        Err(meta.error("unsupported action attribute"))
    })?;
    let id = id.ok_or_else(|| syn::Error::new_spanned(attribute, "missing id"))?;
    let label = label.ok_or_else(|| syn::Error::new_spanned(attribute, "missing label"))?;
    Ok((id, label))
}

fn reserved_name(item: &TraitItem) -> Option<&'static str> {
    let identifier = match item {
        TraitItem::Const(item) => &item.ident,
        TraitItem::Fn(item) => &item.sig.ident,
        TraitItem::Type(item) => &item.ident,
        _ => return None,
    };
    [
        "__DOMAIN_ACTIONS",
        "__DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE",
    ]
    .into_iter()
    .find(|name| identifier == name)
}
