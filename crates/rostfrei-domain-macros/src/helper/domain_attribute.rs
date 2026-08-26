use syn::{Attribute, Result};

pub fn is_helper(attribute: &Attribute) -> bool {
    attribute.path().is_ident("rostfrei") || attribute.path().is_ident("domain")
}

pub fn locate(attributes: &[Attribute]) -> Result<&Attribute> {
    let mut found: Option<(&Attribute, &str)> = None;
    for attribute in attributes {
        let name = if attribute.path().is_ident("rostfrei") {
            "rostfrei"
        } else if attribute.path().is_ident("domain") {
            "domain"
        } else {
            continue;
        };
        if let Some((_, previous)) = found {
            let message = if previous == name {
                "duplicate domain attribute".to_owned()
            } else {
                "`rostfrei` and `domain` helper attributes cannot be used together".to_owned()
            };
            return Err(syn::Error::new_spanned(attribute, message));
        }
        found = Some((attribute, name));
    }
    found
        .map(|(attribute, _)| attribute)
        .ok_or_else(|| syn::Error::new(proc_macro2::Span::call_site(), "missing domain attribute"))
}
