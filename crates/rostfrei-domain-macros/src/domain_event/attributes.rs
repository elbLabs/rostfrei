use syn::{Attribute, LitInt, LitStr, Result};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub schema_version: Option<LitInt>,
}

impl Attributes {
    pub fn parse(attributes: &[Attribute]) -> Result<Self> {
        let domain = crate::helper::domain_attribute::locate(attributes)?;
        let mut id = None;
        let mut label = None;
        let mut schema_version = None;
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
            if meta.path.is_ident("schema_version") {
                if schema_version.is_some() {
                    return Err(meta.error("duplicate schema_version"));
                }
                schema_version = Some(meta.value()?.parse::<LitInt>()?);
                return Ok(());
            }
            Err(meta.error("unsupported domain attribute"))
        })?;
        let id = id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?;
        let label = label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?;
        Ok(Self {
            id,
            label,
            schema_version,
        })
    }
}
