use syn::{Attribute, LitStr, Result};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub code: LitStr,
    pub message: LitStr,
}

impl Attributes {
    pub fn parse(attributes: &[Attribute]) -> Result<Self> {
        let domain = crate::helper::domain_attribute::locate(attributes)?;
        let mut id = None;
        let mut label = None;
        let mut code = None;
        let mut message = None;
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
            if meta.path.is_ident("code") {
                if code.is_some() {
                    return Err(meta.error("duplicate code"));
                }
                code = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            if meta.path.is_ident("message") {
                if message.is_some() {
                    return Err(meta.error("duplicate message"));
                }
                message = Some(meta.value()?.parse::<LitStr>()?);
                return Ok(());
            }
            Err(meta.error("unsupported domain attribute"))
        })?;
        let id = id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?;
        let label = label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?;
        let code = code.ok_or_else(|| syn::Error::new_spanned(domain, "missing code"))?;
        let message = message.ok_or_else(|| syn::Error::new_spanned(domain, "missing message"))?;
        Ok(Self {
            id,
            label,
            code,
            message,
        })
    }
}
