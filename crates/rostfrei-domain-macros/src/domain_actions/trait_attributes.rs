use syn::{LitStr, TraitItem};

use super::action::Action;

#[derive(Clone, Copy)]
pub enum RaisesPolicy {
    Forbidden,
    Required,
}

pub fn extract(items: &mut [TraitItem], raises_policy: RaisesPolicy) -> syn::Result<Vec<Action>> {
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
        let position = {
            let mut positions = method
                .attrs
                .iter()
                .enumerate()
                .filter(|(_, attribute)| attribute.path().is_ident("action"));
            let Some((position, _)) = positions.next() else {
                return Err(syn::Error::new_spanned(
                    &method.sig.ident,
                    "domain action contract methods require exactly one action attribute",
                ));
            };
            if let Some((_, duplicate)) = positions.next() {
                return Err(syn::Error::new_spanned(
                    duplicate,
                    "duplicate action attribute",
                ));
            }
            position
        };
        let attribute = method.attrs.remove(position);
        let (id, label, raises) = parse(&attribute, raises_policy)?;
        actions.push(Action {
            id,
            label,
            raises,
            syntax: method.sig.clone(),
            signature: None,
        });
    }
    Ok(actions)
}

fn parse(
    attribute: &syn::Attribute,
    raises_policy: RaisesPolicy,
) -> syn::Result<(LitStr, LitStr, Vec<syn::Path>)> {
    let mut id = None;
    let mut label = None;
    let mut raises = None;
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
        if meta.path.is_ident("raises") {
            if matches!(raises_policy, RaisesPolicy::Forbidden) {
                return Err(
                    meta.error("raises is only supported by executable aggregate action contracts")
                );
            }
            if raises.is_some() {
                return Err(meta.error("duplicate raises"));
            }
            raises = Some(crate::helper::event_paths::parse(meta.value()?)?);
            return Ok(());
        }
        Err(meta.error("unsupported action attribute"))
    })?;
    let id = id.ok_or_else(|| syn::Error::new_spanned(attribute, "missing id"))?;
    let label = label.ok_or_else(|| syn::Error::new_spanned(attribute, "missing label"))?;
    let raises = match raises_policy {
        RaisesPolicy::Forbidden => Vec::new(),
        RaisesPolicy::Required => match raises {
            None => return Err(syn::Error::new_spanned(attribute, "missing raises")),
            Some(raises) if raises.is_empty() => {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "executable aggregate actions must declare at least one raised event",
                ));
            }
            Some(raises) => raises,
        },
    };
    Ok((id, label, raises))
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
