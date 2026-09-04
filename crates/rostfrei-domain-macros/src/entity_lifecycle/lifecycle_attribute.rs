use syn::{Attribute, Ident};

pub struct LifecycleAttribute {
    pub initial: Ident,
}

pub fn parse(attributes: &[Attribute]) -> syn::Result<LifecycleAttribute> {
    let lifecycles: Vec<_> = attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("lifecycle"))
        .collect();
    let Some(lifecycle) = lifecycles.first() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "missing lifecycle attribute",
        ));
    };
    if let Some(duplicate) = lifecycles.get(1) {
        return Err(syn::Error::new_spanned(
            duplicate,
            "duplicate lifecycle attribute",
        ));
    }

    let mut initial = None;
    lifecycle.parse_nested_meta(|meta| {
        if meta.path.is_ident("initial") {
            if initial.is_some() {
                return Err(meta.error("duplicate initial state"));
            }
            initial = Some(meta.value()?.parse::<Ident>()?);
            return Ok(());
        }
        Err(meta.error("unsupported lifecycle attribute"))
    })?;

    Ok(LifecycleAttribute {
        initial: initial
            .ok_or_else(|| syn::Error::new_spanned(lifecycle, "missing initial state"))?,
    })
}
