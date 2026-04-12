use syn::{Attribute, Result};

pub fn locate(attributes: &[Attribute]) -> Result<&Attribute> {
    let mut domains = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("domain"));
    let domain = domains.next().ok_or_else(|| {
        syn::Error::new(proc_macro2::Span::call_site(), "missing domain attribute")
    })?;
    if let Some(duplicate) = domains.next() {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate domain attribute",
        ));
    }
    Ok(domain)
}
