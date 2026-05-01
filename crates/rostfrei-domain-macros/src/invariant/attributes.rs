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
            let target = if meta.path.is_ident("id") {
                &mut id
            } else if meta.path.is_ident("label") {
                &mut label
            } else {
                return Err(meta.error("unsupported domain_invariant argument"));
            };
            if target.is_some() {
                return Err(meta.error(if meta.path.is_ident("id") {
                    "duplicate id"
                } else {
                    "duplicate label"
                }));
            }
            *target = Some(meta.value()?.parse::<LitStr>()?);
            Ok(())
        });
        parser.parse2(arguments.clone())?;
        Ok(Self {
            id: id.ok_or_else(|| syn::Error::new_spanned(arguments, "missing id"))?,
            label: label.ok_or_else(|| syn::Error::new_spanned(arguments, "missing label"))?,
        })
    }
}
