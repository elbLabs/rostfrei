use proc_macro2::TokenStream;
use syn::parse::Parser;
use syn::{LitStr, Result};

pub struct Attributes {
    pub id: LitStr,
    pub label: LitStr,
}

impl Attributes {
    pub fn parse(arguments: &TokenStream) -> Result<Self> {
        let mut id = None;
        let mut label = None;
        let parser = syn::meta::parser(|meta| {
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
            Err(meta.error("unsupported domain_action argument"))
        });
        parser.parse2(arguments.clone())?;
        let id = id.ok_or_else(|| syn::Error::new_spanned(arguments, "missing id"))?;
        let label = label.ok_or_else(|| syn::Error::new_spanned(arguments, "missing label"))?;
        Ok(Self { id, label })
    }
}
