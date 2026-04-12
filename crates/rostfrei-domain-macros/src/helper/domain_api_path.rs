use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use syn::{Path, Result};

pub fn resolve() -> Result<Path> {
    match crate_name("rostfrei") {
        Ok(found) => path_for("rostfrei", found),
        Err(rostfrei_error) => match crate_name("rostfrei-domain") {
            Ok(found) => path_for("rostfrei-domain", found),
            Err(domain_error) => Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "could not resolve the Rostfrei domain API; add a dependency on package \
                     `rostfrei` or `rostfrei-domain` (rostfrei: {rostfrei_error}; \
                     rostfrei-domain: {domain_error})"
                ),
            )),
        },
    }
}

fn path_for(package: &str, found: FoundCrate) -> Result<Path> {
    match found {
        FoundCrate::Itself if package == "rostfrei" => Ok(syn::parse_quote!(::rostfrei)),
        FoundCrate::Itself => Ok(syn::parse_quote!(crate)),
        FoundCrate::Name(name) => {
            let name = if package == "rostfrei-domain" && name == "rostfrei_domain" {
                "domain"
            } else {
                &name
            };
            syn::parse_str(&format!("::{name}")).map_err(|error| {
                syn::Error::new(
                    Span::call_site(),
                    format!(
                        "package `{package}` resolved to invalid crate identifier `{name}`: {error}"
                    ),
                )
            })
        }
    }
}
