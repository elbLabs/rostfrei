use syn::{LitStr, Variant};

pub struct TransitionAttribute {
    pub id: LitStr,
    pub label: LitStr,
}

pub fn parse(variant: &Variant) -> syn::Result<TransitionAttribute> {
    let transitions: Vec<_> = variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("transition"))
        .collect();
    let Some(transition) = transitions.first() else {
        return Err(syn::Error::new_spanned(
            &variant.ident,
            "StateTransition variants require exactly one transition attribute",
        ));
    };
    if let Some(duplicate) = transitions.get(1) {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate transition attribute",
        ));
    }

    let mut id = None;
    let mut label = None;
    transition.parse_nested_meta(|meta| {
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
        Err(meta.error("unsupported state transition metadata"))
    })?;

    Ok(TransitionAttribute {
        id: id.ok_or_else(|| syn::Error::new_spanned(transition, "missing transition id"))?,
        label: label
            .ok_or_else(|| syn::Error::new_spanned(transition, "missing transition label"))?,
    })
}
