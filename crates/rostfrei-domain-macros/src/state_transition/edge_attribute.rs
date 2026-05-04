use syn::{Ident, LitStr, Variant};

pub struct EdgeAttribute {
    pub id: LitStr,
    pub label: LitStr,
    pub from: Ident,
    pub to: Ident,
}

pub fn parse(variant: &Variant) -> syn::Result<EdgeAttribute> {
    let edges: Vec<_> = variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("edge"))
        .collect();
    let Some(edge) = edges.first() else {
        return Err(syn::Error::new_spanned(
            &variant.ident,
            "StateTransition variants require exactly one edge attribute",
        ));
    };
    if let Some(duplicate) = edges.get(1) {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate edge attribute",
        ));
    }

    let mut id = None;
    let mut label = None;
    let mut from = None;
    let mut to = None;
    edge.parse_nested_meta(|meta| {
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
        if meta.path.is_ident("from") {
            if from.is_some() {
                return Err(meta.error("duplicate from state"));
            }
            from = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        if meta.path.is_ident("to") {
            if to.is_some() {
                return Err(meta.error("duplicate to state"));
            }
            to = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        Err(meta.error("unsupported state transition edge metadata"))
    })?;

    Ok(EdgeAttribute {
        id: id.ok_or_else(|| syn::Error::new_spanned(edge, "missing transition id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(edge, "missing transition label"))?,
        from: from.ok_or_else(|| syn::Error::new_spanned(edge, "missing from state"))?,
        to: to.ok_or_else(|| syn::Error::new_spanned(edge, "missing to state"))?,
    })
}
