use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use syn::{Path, Result};

pub fn resolve_optional() -> Result<Option<Path>> {
    match crate_name("rostfrei") {
        Ok(found) => facade_path(found).map(Some),
        Err(_) => match crate_name("rostfrei-domain-runtime") {
            Ok(found) => dependency_path("rostfrei-domain-runtime", found).map(Some),
            Err(_) => Ok(None),
        },
    }
}

fn facade_path(found: FoundCrate) -> Result<Path> {
    let root = dependency_path("rostfrei", found)?;
    syn::parse2(quote::quote!(#root::__private::domain_runtime))
}

fn dependency_path(package: &str, found: FoundCrate) -> Result<Path> {
    match found {
        FoundCrate::Itself if package == "rostfrei" => Ok(syn::parse_quote!(::rostfrei)),
        FoundCrate::Itself => Ok(syn::parse_quote!(crate)),
        FoundCrate::Name(name) => syn::parse_str(&format!("::{name}")).map_err(|error| {
            syn::Error::new(
                Span::call_site(),
                format!(
                    "package `{package}` resolved to invalid crate identifier `{name}`: {error}"
                ),
            )
        }),
    }
}
