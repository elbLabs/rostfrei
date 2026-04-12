use syn::{Attribute, LitStr, TraitItemFn};

pub struct DecisionAttribute {
    pub id: LitStr,
    pub label: LitStr,
}

pub fn extract(method: &mut TraitItemFn) -> syn::Result<DecisionAttribute> {
    let positions: Vec<_> = method
        .attrs
        .iter()
        .enumerate()
        .filter(|(_, attribute)| attribute.path().is_ident("decision"))
        .map(|(position, _)| position)
        .collect();

    if positions.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.ident,
            "domain decision contract methods require exactly one decision attribute",
        ));
    }
    if positions.len() > 1 {
        return Err(syn::Error::new_spanned(
            &method.attrs[positions[1]],
            "duplicate decision attribute",
        ));
    }

    parse(&method.attrs.remove(positions[0]))
}

fn parse(attribute: &Attribute) -> syn::Result<DecisionAttribute> {
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
        Err(meta.error("unsupported decision attribute metadata"))
    })?;

    Ok(DecisionAttribute {
        id: id.ok_or_else(|| syn::Error::new_spanned(attribute, "missing id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(attribute, "missing label"))?,
    })
}
