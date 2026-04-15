use proc_macro2::Span;
use syn::{Attribute, LitInt, LitStr, Result, Type, TypePath};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
    pub owner: TypePath,
    pub rejection: Option<Type>,
    pub schema_version: LitInt,
    pub json: bool,
    pub runtime: bool,
}

impl Attributes {
    pub fn parse(attributes: &[Attribute]) -> Result<Self> {
        let domain = crate::helper::domain_attribute::locate(attributes)?;
        let mut id = None;
        let mut label = None;
        let mut owner = None;
        let mut rejection = None;
        let mut schema_version = None;
        let mut json = false;
        let mut runtime = false;
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
            if meta.path.is_ident("json") {
                if json {
                    return Err(meta.error("duplicate json"));
                }
                json = true;
                return Ok(());
            }
            if meta.path.is_ident("runtime") {
                if runtime {
                    return Err(meta.error("duplicate runtime"));
                }
                runtime = true;
                return Ok(());
            }
            if meta.path.is_ident("rejection") {
                if rejection.is_some() {
                    return Err(meta.error("duplicate rejection"));
                }
                rejection = Some(meta.value()?.parse::<Type>()?);
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
        Ok(Self {
            id: id.ok_or_else(|| syn::Error::new_spanned(domain, "missing id"))?,
            label: label.ok_or_else(|| syn::Error::new_spanned(domain, "missing label"))?,
            owner: owner.ok_or_else(|| syn::Error::new_spanned(domain, "missing owner"))?,
            rejection,
            schema_version: schema_version.unwrap_or_else(|| LitInt::new("1", Span::call_site())),
            json,
            runtime,
        })
    }
}
