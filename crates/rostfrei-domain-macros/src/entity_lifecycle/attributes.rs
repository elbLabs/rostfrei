use syn::{Attribute, LitStr};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
}

pub fn parse(attributes: &[Attribute]) -> syn::Result<Attributes> {
    let domain = crate::helper::domain_attribute::locate(attributes)?;
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
        Err(meta.error("unsupported lifecycle domain attribute"))
    })?;

    Ok(Attributes {
        id: id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?,
    })
}
