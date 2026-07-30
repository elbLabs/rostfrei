use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, LitInt, LitStr};

use crate::support::set_once;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    if !matches!(input.data, Data::Struct(_)) {
        return Err(Error::new_spanned(
            input,
            "ReadModel can only be derived for a struct",
        ));
    }
    let mut id: Option<LitStr> = None;
    let mut version: Option<LitInt> = None;
    for attribute in input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("read_model"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                set_once(&mut id, meta.value()?.parse()?, &meta.path, "id")
            } else if meta.path.is_ident("version") {
                set_once(&mut version, meta.value()?.parse()?, &meta.path, "version")
            } else {
                Err(meta.error("expected `id` or `version` in read_model attribute"))
            }
        })?;
    }
    let id = id.ok_or_else(|| Error::new_spanned(input, "missing read_model `id`"))?;
    let value = id.value();
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Error::new_spanned(
            id,
            "read-model id must be a lowercase kebab-case name of 1–64 bytes",
        ));
    }
    let version =
        version.ok_or_else(|| Error::new_spanned(input, "missing read_model `version`"))?;
    let version_value: u32 = version.base10_parse()?;
    if version_value == 0 {
        return Err(Error::new_spanned(
            version,
            "read-model version must be greater than zero",
        ));
    }
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics crate::__rostfrei_macro_support::ReadModel for #ident #type_generics #where_clause {
            const NAME: &'static str = #id;
            const SCHEMA_VERSION: u32 = #version_value;
        }
    })
}
