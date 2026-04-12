use syn::{LitStr, Variant};

pub struct StateAttribute {
    pub id: LitStr,
    pub label: LitStr,
}

pub fn parse(variant: &Variant) -> syn::Result<StateAttribute> {
    let domains: Vec<_> = variant
        .attrs
        .iter()
        .filter(|attribute| crate::helper::domain_attribute::is_helper(attribute))
        .collect();
    let Some(domain) = domains.first() else {
        return Err(syn::Error::new_spanned(
            &variant.ident,
            "EntityLifecycle state variants require exactly one domain attribute",
        ));
    };
    if let Some(duplicate) = domains.get(1) {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate state domain attribute",
        ));
    }

    let mut id = None;
    let mut label = None;
    domain.parse_nested_meta(|meta| {
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
        Err(meta.error("unsupported lifecycle state metadata"))
    })?;

    Ok(StateAttribute {
        id: id.ok_or_else(|| syn::Error::new_spanned(domain, "missing state id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(domain, "missing state label"))?,
    })
}
