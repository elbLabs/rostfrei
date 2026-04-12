use syn::{Attribute, Ident, LitStr, TypePath};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub owner: TypePath,
    pub initial: Ident,
}

pub fn parse(attributes: &[Attribute]) -> syn::Result<Attributes> {
    let domain = crate::helper::domain_attribute::locate(attributes)?;
    let mut id = None;
    let mut label = None;
    let mut owner = None;
    let mut initial = None;

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
        if meta.path.is_ident("owner") {
            if owner.is_some() {
                return Err(meta.error("duplicate owner"));
            }
            owner = Some(meta.value()?.parse::<TypePath>()?);
            return Ok(());
        }
        if meta.path.is_ident("initial") {
            if initial.is_some() {
                return Err(meta.error("duplicate initial"));
            }
            initial = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        Err(meta.error("unsupported lifecycle domain attribute"))
    })?;

    Ok(Attributes {
        id: id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?,
        label: label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?,
        owner: owner.ok_or_else(|| syn::Error::new_spanned(domain, "missing owner"))?,
        initial: initial.ok_or_else(|| syn::Error::new_spanned(domain, "missing initial"))?,
    })
}
