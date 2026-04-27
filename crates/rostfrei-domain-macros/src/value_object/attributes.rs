use syn::{Attribute, LitStr, Path, Result, TypePath};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub owner: TypePath,
    pub actions: Vec<Path>,
}

impl Attributes {
    pub fn parse(attributes: &[Attribute]) -> Result<Self> {
        let domain = crate::helper::domain_attribute::locate(attributes)?;
        let mut id = None;
        let mut label = None;
        let mut owner = None;
        let mut actions = None;
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
            if meta.path.is_ident("actions") {
                if actions.is_some() {
                    return Err(meta.error("duplicate actions"));
                }
                actions = Some(crate::helper::action_paths::parse(meta.value()?)?);
                return Ok(());
            }
            Err(meta.error("unsupported domain attribute"))
        })?;
        let id = id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?;
        let label = label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?;
        let owner = owner.ok_or_else(|| syn::Error::new_spanned(domain, "missing owner"))?;
        Ok(Self {
            id,
            label,
            owner,
            actions: actions.unwrap_or_default(),
        })
    }
}
