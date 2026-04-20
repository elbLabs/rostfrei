use syn::{LitStr, Variant};

pub struct OutcomeAttribute {
    pub id: LitStr,
    pub label: LitStr,
}

pub fn parse(variant: &Variant) -> syn::Result<OutcomeAttribute> {
    let attributes: Vec<_> = variant
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("outcome"))
        .collect();
    let Some(attribute) = attributes.first() else {
        return Err(syn::Error::new_spanned(
            &variant.ident,
            "DecisionOutcome variants require exactly one outcome attribute",
        ));
    };
    if let Some(duplicate) = attributes.get(1) {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate outcome attribute",
        ));
    }

    let mut id = None;
    let mut label = None;
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            if id.is_some() {
                return Err(meta.error("duplicate outcome id"));
            }
            id = Some(meta.value()?.parse::<LitStr>()?);
            return Ok(());
        }
        if meta.path.is_ident("label") {
            if label.is_some() {
                return Err(meta.error("duplicate outcome label"));
            }
            label = Some(meta.value()?.parse::<LitStr>()?);
            return Ok(());
        }
        Err(meta.error("unsupported outcome attribute key"))
    })?;

    Ok(OutcomeAttribute {
        id: id.ok_or_else(|| syn::Error::new_spanned(attribute, "missing outcome id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(attribute, "missing outcome label"))?,
    })
}
