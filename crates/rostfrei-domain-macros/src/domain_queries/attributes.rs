use syn::{ImplItem, LitStr, Visibility};

use super::signature::ParsedSignature;

pub struct Query {
    pub id: LitStr,
    pub label: LitStr,
    pub visibility: Visibility,
    pub syntax: syn::Signature,
    pub signature: Option<ParsedSignature>,
}

pub fn extract(items: &mut [ImplItem]) -> syn::Result<Vec<Query>> {
    let mut queries = Vec::new();
    for item in items {
        match item {
            ImplItem::Fn(method) => {
                let positions: Vec<_> = method
                    .attrs
                    .iter()
                    .enumerate()
                    .filter(|(_, attribute)| attribute.path().is_ident("query"))
                    .map(|(position, _)| position)
                    .collect();
                if positions.len() > 1 {
                    return Err(syn::Error::new_spanned(
                        &method.attrs[positions[1]],
                        "duplicate query attribute",
                    ));
                }
                if let Some(position) = positions.first().copied() {
                    let attribute = method.attrs.remove(position);
                    let (id, label) = parse(&attribute)?;
                    queries.push(Query {
                        id,
                        label,
                        visibility: method.vis.clone(),
                        syntax: method.sig.clone(),
                        signature: None,
                    });
                }
            }
            item => {
                if let Some(attribute) = attributes(item)
                    .iter()
                    .find(|attribute| attribute.path().is_ident("query"))
                {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "query may only be applied to associated functions",
                    ));
                }
            }
        }
    }
    Ok(queries)
}

fn parse(attribute: &syn::Attribute) -> syn::Result<(LitStr, LitStr)> {
    let mut id = None;
    let mut label = None;
    attribute.parse_nested_meta(|meta| {
        let target = if meta.path.is_ident("id") {
            &mut id
        } else if meta.path.is_ident("label") {
            &mut label
        } else {
            return Err(meta.error("unsupported query attribute"));
        };
        if target.is_some() {
            return Err(meta.error(if meta.path.is_ident("id") {
                "duplicate id"
            } else {
                "duplicate label"
            }));
        }
        *target = Some(meta.value()?.parse::<LitStr>()?);
        Ok(())
    })?;
    Ok((
        id.ok_or_else(|| syn::Error::new_spanned(attribute, "missing id"))?,
        label.ok_or_else(|| syn::Error::new_spanned(attribute, "missing label"))?,
    ))
}

fn attributes(item: &ImplItem) -> &[syn::Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        _ => &[],
    }
}
