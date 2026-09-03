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

pub fn registry_path() -> Path {
    syn::parse_quote!(crate::__rostfrei_macro_support::__private::registry)
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
