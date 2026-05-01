use proc_macro_crate::{FoundCrate, crate_name};
use quote::quote;
use syn::{Attribute, DeriveInput, Error, Path};

pub fn rostfrei_attributes(attributes: &[Attribute]) -> impl Iterator<Item = &Attribute> {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("rostfrei"))
}

pub fn set_once<T>(slot: &mut Option<T>, value: T, path: &Path, name: &str) -> syn::Result<()> {
    if slot.is_some() {
        return Err(Error::new_spanned(
            path,
            format!("duplicate `{name}` attribute"),
        ));
    }

    *slot = Some(value);
    Ok(())
}

pub fn required<T>(
    value: Option<T>,
    input: &DeriveInput,
    message: &str,
    errors: &mut Errors,
) -> Option<T> {
    if value.is_none() {
        errors.push(Error::new(input.ident.span(), message));
    }
    value
}

pub fn registry_path() -> syn::Result<Path> {
    if let Ok(found) = crate_name("rostfrei-registry") {
        found_crate_path("rostfrei-registry", found)
    } else {
        let facade = dependency_path("rostfrei")?;
        syn::parse2(quote!(#facade::__private::registry))
    }
}

fn dependency_path(package: &str) -> syn::Result<Path> {
    let found = crate_name(package).map_err(|error| {
        Error::new(
            proc_macro2::Span::call_site(),
            format!("could not resolve the `{package}` dependency: {error}"),
        )
    })?;

    found_crate_path(package, found)
}

fn found_crate_path(package: &str, found: FoundCrate) -> syn::Result<Path> {
    match found {
        FoundCrate::Itself if package == "rostfrei" => syn::parse_str("::rostfrei"),
        FoundCrate::Itself => syn::parse_str("crate"),
        FoundCrate::Name(name) => syn::parse_str(&format!("::{name}")),
    }
}

#[derive(Default)]
pub struct Errors(Option<Error>);

impl Errors {
    pub fn push(&mut self, error: Error) {
        if let Some(errors) = &mut self.0 {
            errors.combine(error);
        } else {
            self.0 = Some(error);
        }
    }

    pub fn finish(self) -> syn::Result<()> {
        self.0.map_or(Ok(()), Err)
    }
}
